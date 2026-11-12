//! Local axum server for the HTTP engine tests (plan 03 §10), plain HTTP or self-signed TLS.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{FromRequest, Multipart, Path as UrlPath, Query, Request};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use base64::Engine as _;
use serde_json::{json, Value};

const DRIP_INTERVAL: Duration = Duration::from_millis(100);
const CHUNK_BYTES: usize = 64 * 1024;

pub struct TestServer {
    pub addr: SocketAddr,
    pub scheme: &'static str,
    hits: Arc<AtomicUsize>,
}

impl TestServer {
    /// Requests received so far.
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}://{}{path}", self.scheme, self.addr)
    }
}

pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http")
}

/// Starts the server on `127.0.0.1:0` in the current tokio runtime.
pub async fn spawn() -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let app = router(Arc::clone(&hits));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    TestServer {
        addr,
        scheme: "http",
        hits,
    }
}

/// The same router over HTTPS with a self-signed certificate for `127.0.0.1`.
pub async fn spawn_tls() -> TestServer {
    let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let config = axum_server::tls_rustls::RustlsConfig::from_pem(
        certified.cert.pem().into_bytes(),
        certified.signing_key.serialize_pem().into_bytes(),
    )
    .await
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = axum_server::from_tcp_rustls(listener, config).unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let app = router(Arc::clone(&hits));
    tokio::spawn(async move { server.serve(app.into_make_service()).await.unwrap() });
    TestServer {
        addr,
        scheme: "https",
        hits,
    }
}

fn router(hits: Arc<AtomicUsize>) -> Router {
    let count = move |request: Request, next: axum::middleware::Next| {
        hits.fetch_add(1, Ordering::SeqCst);
        next.run(request)
    };
    Router::new()
        .route("/echo", any(echo))
        .route("/status/{code}", any(status))
        .route("/redirect/{n}", get(redirect))
        .route("/redirect-to", any(redirect_to))
        .route("/delay/{ms}", get(delay))
        .route("/drip", get(drip))
        .route("/bytes/{n}", get(bytes))
        .route("/cookies/set", get(set_cookies))
        .route("/cookies", get(cookies))
        .route("/gzip", get(gzip))
        .route("/fixture/{name}", get(fixture))
        .layer(axum::middleware::from_fn(count))
}

fn header_list(headers: &HeaderMap) -> Vec<Value> {
    headers
        .iter()
        .map(|(name, value)| json!([name.as_str(), String::from_utf8_lossy(value.as_bytes())]))
        .collect()
}

async fn echo(request: Request) -> Json<Value> {
    let method = request.method().to_string();
    let query = request.uri().query().unwrap_or_default().to_string();
    let headers = header_list(request.headers());
    let is_multipart = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("multipart/form-data"));
    let (body, parts) = if is_multipart {
        let multipart = Multipart::from_request(request, &()).await.unwrap();
        (Vec::new(), multipart_parts(multipart).await)
    } else {
        let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
            .await
            .unwrap();
        (bytes.to_vec(), Vec::new())
    };
    Json(json!({
        "method": method,
        "query": query,
        "headers": headers,
        "body": base64::engine::general_purpose::STANDARD.encode(body),
        "parts": parts,
    }))
}

async fn multipart_parts(mut multipart: Multipart) -> Vec<Value> {
    let mut parts = Vec::new();
    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().map(str::to_string);
        let data = field.bytes().await.unwrap();
        parts.push(json!({
            "name": name,
            "file_name": file_name,
            "size": data.len(),
            "body": base64::engine::general_purpose::STANDARD.encode(&data),
        }));
    }
    parts
}

async fn status(UrlPath(code): UrlPath<u16>) -> StatusCode {
    StatusCode::from_u16(code).unwrap()
}

fn found(location: &str) -> Response {
    (
        StatusCode::FOUND,
        [(header::LOCATION, location.to_string())],
    )
        .into_response()
}

async fn redirect(UrlPath(n): UrlPath<u32>) -> Response {
    if n == 0 {
        "done".into_response()
    } else {
        found(&format!("/redirect/{}", n - 1))
    }
}

#[derive(serde::Deserialize)]
struct Target {
    url: String,
}

async fn redirect_to(Query(target): Query<Target>) -> Response {
    found(&target.url)
}

async fn delay(UrlPath(ms): UrlPath<u64>) -> &'static str {
    tokio::time::sleep(Duration::from_millis(ms)).await;
    "late"
}

async fn drip() -> Body {
    let stream = futures_util::stream::unfold((), |()| async {
        tokio::time::sleep(DRIP_INTERVAL).await;
        Some((Ok::<_, std::io::Error>(Bytes::from_static(b"drip\n")), ()))
    });
    Body::from_stream(stream)
}

async fn bytes(UrlPath(n): UrlPath<usize>) -> Body {
    let stream = futures_util::stream::unfold(n, |left| async move {
        (left > 0).then(|| {
            let size = left.min(CHUNK_BYTES);
            (
                Ok::<_, std::io::Error>(Bytes::from(vec![b'a'; size])),
                left - size,
            )
        })
    });
    Body::from_stream(stream)
}

async fn set_cookies(Query(pairs): Query<Vec<(String, String)>>) -> Response {
    let mut response = "set".into_response();
    for (name, value) in pairs {
        let cookie =
            format!("{name}={value}; Path=/; HttpOnly; Expires=Wed, 21 Oct 2099 07:28:00 GMT");
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie.parse().unwrap());
    }
    response
}

async fn cookies(headers: HeaderMap) -> String {
    headers
        .get(header::COOKIE)
        .map(|value| value.to_str().unwrap().to_string())
        .unwrap_or_default()
}

async fn gzip() -> Response {
    use std::io::Write as _;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(b"unzipped text").unwrap();
    let body = encoder.finish().unwrap();
    (
        [
            (header::CONTENT_ENCODING, "gzip"),
            (header::CONTENT_TYPE, "text/plain"),
        ],
        body,
    )
        .into_response()
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or_default() {
        "json" => "application/json",
        "xml" => "application/xml",
        "html" => "text/html; charset=utf-8",
        "png" => "image/png",
        "txt" if name == "latin1.txt" => "text/plain; charset=iso-8859-1",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

async fn fixture(UrlPath(name): UrlPath<String>) -> Response {
    match std::fs::read(fixture_dir().join(&name)) {
        Ok(body) => ([(header::CONTENT_TYPE, content_type(&name))], body).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
