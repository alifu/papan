//! One request → a Postman v2.1 item (plan 09 §3). WS and gRPC have no v2.1 shape and are
//! skipped; SOAP becomes a raw XML POST with its version's headers.

use serde_json::{Map, Value};

use super::{auth, body, url, Report};
use crate::interop::postman::schema::{Event, Header, Request, RequestItem, Script};
use crate::interop::postman::types::ReportLevel;
use crate::store::types::{
    KeyValue, RequestContent, RequestKind, Scripts, Settings, SoapSettings, SoapVersion,
    SubscriptionSettings,
};

const TIMEOUT_NOT_EXPORTED: &str = "Request timeouts have no Postman field and were not exported.";

/// `None` when the request kind cannot be written to v2.1 (reported as skipped).
pub fn request_item(
    name: &str,
    content: &RequestContent,
    description: Option<String>,
    path: &str,
    report: &mut Report,
) -> Option<RequestItem> {
    match content.kind {
        RequestKind::Ws => return skip(report, path, "WebSocket"),
        RequestKind::Grpc => return skip(report, path, "gRPC"),
        RequestKind::Http | RequestKind::Graphql | RequestKind::Soap => {}
    }
    if content.subscription != SubscriptionSettings::default() {
        report.add(
            ReportLevel::Info,
            path,
            "GraphQL subscriptions have no Postman v2.1 shape; the subscription was dropped.",
        );
    }
    let mut header: Vec<Header> = content.headers.iter().map(header_of).collect();
    if content.kind == RequestKind::Soap {
        header.extend(soap_headers(&content.soap, &content.headers));
    }
    let request = Request {
        auth: auth::auth(&content.auth, path, report),
        method: Some(content.method.clone()),
        header,
        body: body::body(&content.body, path, report),
        url: url::url(&content.url, &content.disabled_params),
        description,
    };
    Some(RequestItem {
        name: name.to_string(),
        event: events(&content.scripts),
        protocol_profile_behavior: behaviour(&content.settings, path, report),
        request,
        response_count: 0,
    })
}

/// Our scripts → `prerequest` / `test` events; empty scripts are left out.
pub(super) fn events(scripts: &Scripts) -> Vec<Event> {
    let disabled = !scripts.enabled;
    [
        ("prerequest", &scripts.pre_request),
        ("test", &scripts.post_response),
    ]
    .into_iter()
    .filter(|(_, source)| !source.is_empty())
    .map(|(listen, source)| Event {
        listen: listen.to_string(),
        script: Script {
            kind: Some("text/javascript".into()),
            exec: source.split('\n').map(str::to_string).collect(),
        },
        disabled,
    })
    .collect()
}

fn skip(report: &mut Report, path: &str, what: &str) -> Option<RequestItem> {
    let message = format!("{what} requests cannot be exported to Postman v2.1; skipped.");
    report.add(ReportLevel::Skipped, path, message);
    None
}

fn header_of(kv: &KeyValue) -> Header {
    Header {
        key: kv.name.clone(),
        value: kv.value.clone(),
        disabled: !kv.enabled,
        description: None,
    }
}

/// The headers a SOAP send adds, unless the request already sets them.
fn soap_headers(soap: &SoapSettings, existing: &[KeyValue]) -> Vec<Header> {
    let has = |name: &str| existing.iter().any(|kv| kv.name.eq_ignore_ascii_case(name));
    let wanted = match soap.version {
        SoapVersion::V11 => vec![
            ("Content-Type", "text/xml; charset=utf-8".to_string()),
            ("SOAPAction", format!("\"{}\"", soap.action)),
        ],
        SoapVersion::V12 => {
            let action = if soap.action.is_empty() {
                String::new()
            } else {
                format!("; action=\"{}\"", soap.action)
            };
            vec![(
                "Content-Type",
                format!("application/soap+xml; charset=utf-8{action}"),
            )]
        }
    };
    wanted
        .into_iter()
        .filter(|(name, _)| !has(name))
        .map(|(key, value)| Header {
            key: key.to_string(),
            value,
            ..Header::default()
        })
        .collect()
}

/// Only settings that differ from Postman's defaults are written.
fn behaviour(settings: &Settings, path: &str, report: &mut Report) -> Option<Map<String, Value>> {
    if settings.timeout_ms != Settings::default().timeout_ms {
        report.add_once(ReportLevel::Info, path, TIMEOUT_NOT_EXPORTED);
    }
    let map: Map<String, Value> = [
        ("followRedirects", settings.follow_redirects),
        ("strictSSL", settings.verify_tls),
    ]
    .into_iter()
    .filter(|(_, on)| !on)
    .map(|(key, on)| (key.to_string(), Value::Bool(on)))
    .collect();
    (!map.is_empty()).then_some(map)
}
