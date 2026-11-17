//! Where the time of a send went (plan 03b §6.4). Each client records its DNS lookups
//! (`TimedResolver`) and connection establishments (`TimedConnectLayer`, which spans
//! DNS, TCP, TLS and a proxy `CONNECT`) into its own bounded [`TimingLog`]. A send sums
//! the events that started inside its own window.
//!
//! Known limit: two sends running at once on the same client can pick up each other's
//! events. The resolver is skipped for IP-literal hosts (DNS is then 0), and with a
//! proxy the lookup is the proxy's.

use std::collections::VecDeque;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use super::types::Timing;

/// Events kept per client; older ones are dropped.
pub const MAX_EVENTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Lookup,
    Connect,
}

#[derive(Debug, Clone, Copy)]
struct Event {
    kind: Kind,
    started: Instant,
    duration: Duration,
}

/// The lookups and connects of one client, newest last.
#[derive(Debug, Default)]
pub struct TimingLog {
    events: Mutex<VecDeque<Event>>,
}

/// The summed events of one send window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spent {
    pub lookups: Duration,
    pub connects: Duration,
    pub connected: bool,
}

impl TimingLog {
    fn record(&self, kind: Kind, started: Instant) {
        let event = Event {
            kind,
            started,
            duration: started.elapsed(),
        };
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        if events.len() == MAX_EVENTS {
            events.pop_front();
        }
        events.push_back(event);
    }

    /// The events that started at or after `since`.
    pub fn spent_since(&self, since: Instant) -> Spent {
        let events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        events.iter().filter(|event| event.started >= since).fold(
            Spent::default(),
            |spent, event| match event.kind {
                Kind::Lookup => Spent {
                    lookups: spent.lookups + event.duration,
                    ..spent
                },
                Kind::Connect => Spent {
                    connects: spent.connects + event.duration,
                    connected: true,
                    ..spent
                },
            },
        )
    }
}

impl Spent {
    #[must_use]
    pub fn plus(self, other: Spent) -> Spent {
        Spent {
            lookups: self.lookups + other.lookups,
            connects: self.connects + other.connects,
            connected: self.connected || other.connected,
        }
    }
}

fn millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

/// The four phases; they always add up to `total_ms`. A connect includes its lookup,
/// so connect time is connects minus lookups; both are capped by the time to first byte.
pub fn timing(spent: Spent, ttfb_ms: u32, total_ms: u32) -> Timing {
    let ttfb_ms = ttfb_ms.min(total_ms);
    let dns_ms = millis(spent.lookups).min(ttfb_ms);
    let connect_ms = millis(spent.connects)
        .saturating_sub(dns_ms)
        .min(ttfb_ms - dns_ms);
    Timing {
        dns_ms,
        connect_ms,
        wait_ms: ttfb_ms - dns_ms - connect_ms,
        download_ms: total_ms - ttfb_ms,
        reused: !spent.connected,
    }
}

/// `reqwest`'s resolver, timed: `tokio::net::lookup_host`.
#[derive(Debug)]
pub struct TimedResolver(pub Arc<TimingLog>);

impl Resolve for TimedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let log = Arc::clone(&self.0);
        let host = format!("{}:0", name.as_str());
        Box::pin(async move {
            let started = Instant::now();
            let found = tokio::net::lookup_host(host).await;
            log.record(Kind::Lookup, started);
            let addrs: Vec<SocketAddr> = found?.collect();
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// A tower layer for `ClientBuilder::connector_layer` that times each new connection.
#[derive(Debug, Clone)]
pub struct TimedConnectLayer(pub Arc<TimingLog>);

impl<S> tower_layer::Layer<S> for TimedConnectLayer {
    type Service = TimedConnect<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TimedConnect {
            inner,
            log: Arc::clone(&self.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimedConnect<S> {
    inner: S,
    log: Arc<TimingLog>,
}

/// Generic over the request type: reqwest's connector request type cannot be named.
impl<S, R> tower_service::Service<R> for TimedConnect<S>
where
    S: tower_service::Service<R>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: R) -> Self::Future {
        let log = Arc::clone(&self.log);
        let started = Instant::now();
        let connecting = self.inner.call(request);
        Box::pin(async move {
            let connection = connecting.await;
            log.record(Kind::Connect, started);
            connection
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spent(lookups: u64, connects: u64) -> Spent {
        Spent {
            lookups: Duration::from_millis(lookups),
            connects: Duration::from_millis(connects),
            connected: connects > 0,
        }
    }

    #[test]
    fn phases_add_up_to_the_total() {
        let timing = timing(spent(5, 20), 50, 80);

        assert_eq!(
            (timing.dns_ms, timing.connect_ms, timing.wait_ms),
            (5, 15, 30)
        );
        assert_eq!(timing.download_ms, 30);
        assert!(!timing.reused);
    }

    #[test]
    fn phases_are_capped_by_the_time_to_first_byte() {
        let timing = timing(spent(30, 90), 40, 40);

        assert_eq!(timing.dns_ms + timing.connect_ms + timing.wait_ms, 40);
        assert_eq!(timing.download_ms, 0);
    }

    #[test]
    fn no_connect_means_the_connection_was_reused() {
        assert!(timing(spent(0, 0), 3, 4).reused);
    }

    #[test]
    fn only_events_inside_the_window_count_and_the_log_is_bounded() {
        let log = TimingLog::default();
        log.record(Kind::Connect, Instant::now());
        let since = Instant::now();
        for _ in 0..MAX_EVENTS + 3 {
            log.record(Kind::Lookup, Instant::now());
        }

        let spent = log.spent_since(since);

        assert!(!spent.connected);
        assert_eq!(
            log.events.lock().unwrap().len(),
            MAX_EVENTS,
            "oldest dropped"
        );
    }
}
