//! A plain tonic client for the gRPC test server: h2c or our own TLS connector (tonic
//! has no TLS feature here), generic unary calls on `test.v1.Echo`, reflection listings.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use hyper_util::rt::TokioIo;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::net::TcpStream;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::codegen::http::Uri;
use tonic::codegen::Service;
use tonic::transport::{Channel, Endpoint};
use tonic::{Request, Status};
use tonic_prost::ProstCodec;
use tonic_reflection::pb::{v1, v1alpha};

use super::messages::{EchoRequest, EchoResponse};
use super::GrpcServer;
use crate::support::mtls::ClientPem;

pub async fn h2c(server: &GrpcServer) -> Channel {
    Endpoint::from_shared(server.url())
        .unwrap()
        .connect()
        .await
        .unwrap()
}

pub fn path(method: &str) -> PathAndQuery {
    PathAndQuery::try_from(format!("/test.v1.Echo/{method}")).unwrap()
}

pub async fn unary<Req, Resp>(
    channel: &Channel,
    request: Request<Req>,
    method: &str,
) -> Result<Resp, Status>
where
    Req: prost::Message + Send + Sync + 'static,
    Resp: prost::Message + Default + Send + Sync + 'static,
{
    let mut grpc = tonic::client::Grpc::new(channel.clone());
    grpc.ready().await.unwrap();
    let codec = ProstCodec::<Req, Resp>::default();
    grpc.unary(request, path(method), codec)
        .await
        .map(|r| r.into_inner())
}

pub async fn echo(channel: &Channel, message: &str) -> Result<EchoResponse, Status> {
    let request = Request::new(EchoRequest {
        message: message.into(),
    });
    unary(channel, request, "Unary").await
}

pub async fn list_v1(channel: &Channel) -> Result<Vec<String>, Status> {
    use v1::server_reflection_request::MessageRequest;
    use v1::server_reflection_response::MessageResponse;
    let request = v1::ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    };
    let path = "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo";
    let response = reflect::<_, v1::ServerReflectionResponse>(channel, request, path).await?;
    match response.message_response {
        Some(MessageResponse::ListServicesResponse(list)) => {
            Ok(list.service.into_iter().map(|s| s.name).collect())
        }
        other => panic!("unexpected {other:?}"),
    }
}

pub async fn list_v1alpha(channel: &Channel) -> Result<Vec<String>, Status> {
    use v1alpha::server_reflection_request::MessageRequest;
    use v1alpha::server_reflection_response::MessageResponse;
    let request = v1alpha::ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    };
    let path = "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo";
    let response = reflect::<_, v1alpha::ServerReflectionResponse>(channel, request, path).await?;
    match response.message_response {
        Some(MessageResponse::ListServicesResponse(list)) => {
            Ok(list.service.into_iter().map(|s| s.name).collect())
        }
        other => panic!("unexpected {other:?}"),
    }
}

pub async fn reflect<Req, Resp>(
    channel: &Channel,
    request: Req,
    path: &'static str,
) -> Result<Resp, Status>
where
    Req: prost::Message + Send + Sync + 'static,
    Resp: prost::Message + Default + Send + Sync + 'static,
{
    let mut grpc = tonic::client::Grpc::new(channel.clone());
    grpc.ready().await.unwrap();
    let codec = ProstCodec::<Req, Resp>::default();
    let outbound = Request::new(futures_util::stream::iter([request]));
    let mut inbound = grpc
        .streaming(outbound, PathAndQuery::from_static(path), codec)
        .await?
        .into_inner();
    Ok(inbound.message().await?.unwrap())
}

/// Connects TLS itself (tonic has no TLS feature here), ALPN `h2`.
#[derive(Clone)]
pub struct TlsConnector {
    config: Arc<rustls::ClientConfig>,
    addr: SocketAddr,
}

impl Service<Uri> for TlsConnector {
    type Response = TokioIo<tokio_rustls::client::TlsStream<TcpStream>>;
    type Error = io::Error;
    type Future = BoxFuture<'static, io::Result<Self::Response>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> std::task::Poll<io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _uri: Uri) -> Self::Future {
        let (config, addr) = (Arc::clone(&self.config), self.addr);
        Box::pin(async move {
            let tcp = TcpStream::connect(addr).await?;
            let name = ServerName::try_from("127.0.0.1").unwrap();
            let tls = tokio_rustls::TlsConnector::from(config)
                .connect(name, tcp)
                .await?;
            Ok(TokioIo::new(tls))
        })
    }
}

pub async fn tls_channel(
    server: &GrpcServer,
    ca_pem: &str,
    client: Option<&ClientPem>,
) -> Result<Channel, tonic::transport::Error> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_slice(ca_pem.as_bytes()).unwrap())
        .unwrap();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots);
    let mut config = match client {
        Some(pem) => {
            let cert = CertificateDer::from_pem_slice(pem.cert.as_bytes()).unwrap();
            let key = PrivateKeyDer::from_pem_slice(pem.key.as_bytes()).unwrap();
            builder.with_client_auth_cert(vec![cert], key).unwrap()
        }
        None => builder.with_no_client_auth(),
    };
    config.alpn_protocols = vec![b"h2".to_vec()];
    let connector = TlsConnector {
        config: Arc::new(config),
        addr: server.addr,
    };
    let endpoint = format!("http://{}", server.addr);
    Endpoint::from_shared(endpoint)
        .unwrap()
        .connect_with_connector(connector)
        .await
}
