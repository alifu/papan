//! The loopback redirect receiver (plan 03b §5.4). Bound to `127.0.0.1` only; accepts
//! `GET /callback` with the right `Host` and `state`, answers everything with a fixed
//! page, and keeps waiting on anything else so a local page cannot end the flow.
//! The request line is never logged: it carries the authorization code.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use zeroize::Zeroizing;

use super::{conflict, pkce, plain};
use crate::error::AppError;

/// At most this much of a request is read: the request line and the headers.
pub const MAX_REQUEST_BYTES: usize = 8 * 1024;
/// A connection that has not sent its headers by then is dropped.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Connections served at once; more wait in the accept queue.
const MAX_CONNECTIONS: usize = 8;
const CALLBACK_PATH: &str = "/callback";
/// Pending connections the OS queues for the listener.
#[cfg(windows)]
const LISTEN_BACKLOG: i32 = 128;

const DONE_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
    <title>Papan</title></head><body><p>You can close this tab.</p></body></html>";
const NOT_FOUND_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
    <title>Papan</title></head><body><p>Not found.</p></body></html>";
const BAD_REQUEST_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
    <title>Papan</title></head><body><p>This request cannot be used.</p></body></html>";

/// What the provider sent back, once the `state` matched.
#[derive(Clone, PartialEq, Eq)]
pub enum Callback {
    Code(Zeroizing<String>),
    /// The provider's `error` (and `error_description`) as printable text; the flow
    /// masks it and bounds its length.
    Denied(String),
}

impl std::fmt::Debug for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Code(_) => "Callback::Code(..)",
            Self::Denied(_) => "Callback::Denied(..)",
        })
    }
}

/// How one request is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    /// Not `GET /callback` for this listener: `404`, keep waiting.
    NotFound,
    /// Wrong or missing `state`, or neither `code` nor `error`: `400`, keep waiting.
    BadRequest,
    Done(Callback),
}

fn query_value(query: &str, name: &str) -> Option<String> {
    let mut values = url::form_urlencoded::parse(query.as_bytes())
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned());
    let first = values.next()?;
    // A repeated parameter is ambiguous: refuse it.
    values.next().is_none().then_some(first)
}

fn judge(request: &[u8], port: u16, state: &str) -> Verdict {
    let Ok(text) = std::str::from_utf8(request) else {
        return Verdict::NotFound;
    };
    let mut lines = text.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let (method, target, version) = (
        request_line.next().unwrap_or_default(),
        request_line.next().unwrap_or_default(),
        request_line.next().unwrap_or_default(),
    );
    let hosts: Vec<&str> = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim())
        .collect();
    // DNS-rebinding guard: the browser must have been sent to the literal address.
    let host_ok = hosts.len() == 1 && hosts[0] == format!("127.0.0.1:{port}");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let shape_ok = method == "GET"
        && version.starts_with("HTTP/1.")
        && request_line.next().is_none()
        && path == CALLBACK_PATH;
    if !shape_ok || !host_ok {
        return Verdict::NotFound;
    }
    let state_ok = query_value(query, "state").is_some_and(|given| pkce::same(&given, state));
    if !state_ok {
        return Verdict::BadRequest;
    }
    if let Some(error) = query_value(query, "error") {
        let description = query_value(query, "error_description").unwrap_or_default();
        let text = plain(&format!("{error} {description}"));
        return Verdict::Done(Callback::Denied(text));
    }
    match query_value(query, "code") {
        Some(code) if !code.is_empty() => Verdict::Done(Callback::Code(Zeroizing::new(code))),
        _ => Verdict::BadRequest,
    }
}

/// The reply for a verdict: a fixed page, nothing from the request.
fn reply(verdict: &Verdict) -> String {
    let (status, page) = match verdict {
        Verdict::NotFound => ("404 Not Found", NOT_FOUND_PAGE),
        Verdict::BadRequest => ("400 Bad Request", BAD_REQUEST_PAGE),
        Verdict::Done(_) => ("200 OK", DONE_PAGE),
    };
    format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'none'\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n{page}",
        page.len()
    )
}

/// Reads the request line and headers, at most [`MAX_REQUEST_BYTES`].
async fn read_head(stream: &mut TcpStream) -> Option<Zeroizing<Vec<u8>>> {
    let mut head = Zeroizing::new(Vec::with_capacity(1024));
    let mut chunk = Zeroizing::new([0_u8; 1024]);
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let room = MAX_REQUEST_BYTES - head.len();
        if room == 0 {
            return None;
        }
        let limit = room.min(chunk.len());
        let read = stream.read(&mut chunk[..limit]).await.ok()?;
        if read == 0 {
            return None;
        }
        head.extend_from_slice(&chunk[..read]);
    }
    Some(head)
}

async fn serve(mut stream: TcpStream, port: u16, state: Zeroizing<String>) -> Option<Callback> {
    let head = tokio::time::timeout(READ_TIMEOUT, read_head(&mut stream))
        .await
        .ok()
        .flatten();
    let verdict = match head {
        Some(head) => judge(&head, port, &state),
        None => Verdict::NotFound,
    };
    let written = tokio::time::timeout(READ_TIMEOUT, async {
        stream.write_all(reply(&verdict).as_bytes()).await?;
        stream.shutdown().await
    })
    .await;
    if !matches!(written, Ok(Ok(()))) {
        tracing::debug!("the OAuth callback reply could not be written");
    }
    match verdict {
        Verdict::Done(callback) => Some(callback),
        Verdict::NotFound | Verdict::BadRequest => None,
    }
}

/// Binds `address` as is: elsewhere a second bind of a bound port fails.
#[cfg(not(windows))]
async fn bind_exclusive(address: SocketAddr) -> std::io::Result<TcpListener> {
    TcpListener::bind(address).await
}

/// Binds `address` for exclusive use, so no other program can bind the same port with
/// `SO_REUSEADDR` and take the callback.
#[cfg(windows)]
async fn bind_exclusive(address: SocketAddr) -> std::io::Result<TcpListener> {
    use std::os::windows::io::AsRawSocket;

    use socket2::{Domain, Protocol, Socket, Type};
    use windows_sys::Win32::Networking::WinSock::{
        setsockopt, SOCKET, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
    };

    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    let enabled: i32 = 1;
    // SAFETY: the handle is the live socket `socket` owns, and the option value is an
    // `i32` that outlives the call, passed with its exact size.
    let status = unsafe {
        setsockopt(
            socket.as_raw_socket() as SOCKET,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            std::ptr::from_ref(&enabled).cast(),
            std::mem::size_of::<i32>() as i32,
        )
    };
    if status != 0 {
        return Err(std::io::Error::last_os_error());
    }
    socket.bind(&address.into())?;
    socket.listen(LISTEN_BACKLOG)?;
    socket.set_nonblocking(true)?;
    TcpListener::from_std(socket.into())
}

/// The bound socket of one flow. Dropping it closes the port.
#[derive(Debug)]
pub struct Listener {
    socket: TcpListener,
    port: u16,
}

impl Listener {
    /// Binds `127.0.0.1:<port>`; `0` picks a free port. A busy port is a conflict.
    pub async fn bind(port: u16) -> Result<Self, AppError> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let socket = bind_exclusive(address).await.map_err(|source| {
            if source.kind() == std::io::ErrorKind::AddrInUse {
                conflict(format!(
                    "Port {port} is already in use. Close the program that uses it or \
                     choose another redirect port."
                ))
            } else {
                AppError::internal("Could not listen for the sign-in reply.")
                    .with_detail(source.to_string())
            }
        })?;
        let port = socket.local_addr()?.port();
        Ok(Self { socket, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The redirect URI the provider must send the browser to.
    pub fn redirect_uri(&self) -> String {
        redirect_uri(self.port)
    }

    /// Waits for the callback that carries `state`. Never returns for anything else:
    /// the caller bounds it with the flow lifetime and cancellation, and dropping the
    /// future closes the socket and every open connection.
    pub async fn wait(self, state: &str) -> Callback {
        let mut connections: JoinSet<Option<Callback>> = JoinSet::new();
        loop {
            tokio::select! {
                accepted = self.socket.accept(), if connections.len() < MAX_CONNECTIONS => {
                    match accepted {
                        Ok((stream, _)) => {
                            let state = Zeroizing::new(state.to_string());
                            connections.spawn(serve(stream, self.port, state));
                        }
                        // Out of descriptors and the like: do not spin.
                        Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                    }
                }
                Some(served) = connections.join_next() => {
                    if let Ok(Some(callback)) = served {
                        return callback;
                    }
                }
            }
        }
    }
}

/// `http://127.0.0.1:<port>/callback`.
pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

/// What to register with the provider: the fixed port, or (RFC 8252 §7.3) the
/// loopback address without one when any free port is used.
pub fn registered_uri(port: u16) -> String {
    match port {
        0 => format!("http://127.0.0.1{CALLBACK_PATH}"),
        port => redirect_uri(port),
    }
}

#[cfg(test)]
#[path = "listener_tests.rs"]
mod tests;
