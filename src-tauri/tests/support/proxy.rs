//! A forward proxy for the engine tests (plan 03b §10): `CONNECT` tunnels for HTTPS,
//! absolute-form requests for HTTP, optional Basic auth, and a log of every request it
//! received. Each forwarded HTTP request gets `Connection: close`, so every request is
//! logged; `Proxy-Authorization` is never forwarded, as a real proxy does.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MAX_HEAD: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub method: String,
    pub target: String,
    pub proxy_authorization: Option<String>,
}

pub struct TestProxy {
    pub addr: SocketAddr,
    log: Arc<Mutex<Vec<Hit>>>,
}

impl TestProxy {
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn hits(&self) -> Vec<Hit> {
        self.log.lock().unwrap().clone()
    }
}

/// Starts the proxy; with `credentials`, requests without them get `407`.
pub async fn spawn(credentials: Option<(&str, &str)>) -> TestProxy {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    let expected = credentials.map(|(user, password)| {
        let pair = format!("{user}:{password}");
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(pair)
        )
    });
    let shared = Arc::clone(&log);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (log, expected) = (Arc::clone(&shared), expected.clone());
            tokio::spawn(async move {
                let _ = serve(stream, &log, expected.as_deref()).await;
            });
        }
    });
    TestProxy { addr, log }
}

/// Reads up to the end of the request head; returns (head, bytes read past it).
async fn read_head(stream: &mut TcpStream) -> std::io::Result<(String, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buffer.split_off(end + 4);
            return Ok((String::from_utf8_lossy(&buffer).into_owned(), rest));
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 || buffer.len() > MAX_HEAD {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn header<'a>(lines: &[&'a str], name: &str) -> Option<&'a str> {
    lines.iter().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then_some(value.trim())
    })
}

async fn serve(
    mut client: TcpStream,
    log: &Mutex<Vec<Hit>>,
    expected: Option<&str>,
) -> std::io::Result<()> {
    let (head, rest) = read_head(&mut client).await?;
    let lines: Vec<&str> = head.trim_end().split("\r\n").collect();
    let mut request_line = lines[0].split(' ');
    let method = request_line.next().unwrap_or_default().to_string();
    let target = request_line.next().unwrap_or_default().to_string();
    let proxy_authorization = header(&lines[1..], "proxy-authorization").map(str::to_string);
    log.lock().unwrap().push(Hit {
        method: method.clone(),
        target: target.clone(),
        proxy_authorization: proxy_authorization.clone(),
    });
    if expected.is_some() && proxy_authorization.as_deref() != expected {
        let refused = "HTTP/1.1 407 Proxy Authentication Required\r\n\
            Proxy-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\n\
            Connection: close\r\n\r\n";
        return client.write_all(refused.as_bytes()).await;
    }
    if method == "CONNECT" {
        let mut upstream = TcpStream::connect(&target).await?;
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        upstream.write_all(&rest).await?;
        tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
        return Ok(());
    }
    let url = url::Url::parse(&target).map_err(|_| std::io::ErrorKind::InvalidInput)?;
    let authority = format!(
        "{}:{}",
        url.host_str().unwrap_or_default(),
        url.port_or_known_default().unwrap_or(80)
    );
    let mut upstream = TcpStream::connect(authority).await?;
    let skipped = ["proxy-authorization", "proxy-connection", "connection"];
    let forwarded: Vec<&str> = lines[1..]
        .iter()
        .filter(|line| {
            let name = line.split(':').next().unwrap_or_default().trim();
            !skipped.iter().any(|skip| name.eq_ignore_ascii_case(skip))
        })
        .copied()
        .collect();
    let head = format!(
        "{}\r\n{}\r\nConnection: close\r\n\r\n",
        lines[0],
        forwarded.join("\r\n")
    );
    upstream.write_all(head.as_bytes()).await?;
    upstream.write_all(&rest).await?;
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}
