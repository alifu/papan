//! Server reflection (plan 08 §5, §11 D10): one `ServerReflectionInfo` stream on the
//! request's channel with its metadata (servers often require auth). Services are
//! listed, the file of each is asked for, and missing imports are fetched breadth-first.
//! `grpc.reflection.v1` first; `v1alpha` when the server answers `UNIMPLEMENTED`. The
//! client is hand-written over the message types of `tonic-reflection`.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use http::uri::PathAndQuery;
use prost::Message as _;
use prost_reflect::DescriptorPool;
use prost_types::{FileDescriptorProto, FileDescriptorSet};
use tokio::sync::mpsc;
use tonic::metadata::MetadataMap;
use tonic::transport::Channel;
use tonic::{Code, Request, Status, Streaming};
use tonic_prost::ProstCodec;
use tonic_reflection::pb::{v1, v1alpha};

use super::call::code_name;
use super::channel::{client, ready};
use super::connect::GrpcCallSpec;
use crate::error::{AppError, ErrorCode};

pub const MAX_FILES: usize = 500;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
/// Services followed; the rest of a long list is skipped with a warning.
pub const MAX_SERVICES: usize = 200;
/// Warnings kept, each cut to [`MAX_WARNING_CHARS`].
pub const MAX_WARNINGS: usize = 20;
pub const MAX_WARNING_CHARS: usize = 300;
pub const TIME_LIMIT: Duration = Duration::from_secs(30);
const REFLECTION_PREFIX: &str = "grpc.reflection.";
const QUEUE: usize = 4;

/// What one reflection request asks for.
enum Ask {
    Services,
    Symbol(String),
    File(String),
}

/// What the server answered.
enum Answer {
    Services(Vec<String>),
    Files(Vec<Vec<u8>>),
    /// The server's own error for this one request (for example an unknown file).
    Refused(String),
}

/// One open reflection stream: a question in, an answer out.
trait Session: Send {
    async fn ask(&mut self, ask: Ask) -> Result<Answer, Status>;
}

macro_rules! session {
    ($name:ident, $pb:ident, $path:literal) => {
        struct $name {
            tx: mpsc::Sender<$pb::ServerReflectionRequest>,
            responses: Streaming<$pb::ServerReflectionResponse>,
        }

        impl $name {
            fn request(ask: Ask) -> $pb::ServerReflectionRequest {
                use $pb::server_reflection_request::MessageRequest;
                let message = match ask {
                    Ask::Services => MessageRequest::ListServices(String::new()),
                    Ask::Symbol(symbol) => MessageRequest::FileContainingSymbol(symbol),
                    Ask::File(name) => MessageRequest::FileByFilename(name),
                };
                $pb::ServerReflectionRequest {
                    host: String::new(),
                    message_request: Some(message),
                }
            }

            /// Opens the stream with the first question already queued: some servers
            /// send their response headers only with the first answer.
            async fn open(channel: Channel, metadata: MetadataMap) -> Result<Self, Status> {
                let (tx, rx) = mpsc::channel(QUEUE);
                let queued = tx.send(Self::request(Ask::Services)).await;
                queued.map_err(|_| Status::internal("The reflection stream is closed."))?;
                let requests = futures_util::stream::unfold(rx, |mut rx| async {
                    rx.recv().await.map(|request| (request, rx))
                });
                let mut request = Request::new(requests);
                *request.metadata_mut() = metadata;
                let mut grpc = client(channel);
                ready(&mut grpc).await?;
                let path = PathAndQuery::from_static($path);
                let codec = ProstCodec::<
                                                                    $pb::ServerReflectionRequest,
                                                                    $pb::ServerReflectionResponse,
                                                                >::default();
                let responses = grpc.streaming(request, path, codec).await?.into_inner();
                Ok(Self { tx, responses })
            }

            async fn answer(&mut self) -> Result<Answer, Status> {
                use $pb::server_reflection_response::MessageResponse;
                let response = self.responses.message().await?;
                let message = response.and_then(|response| response.message_response);
                match message {
                    Some(MessageResponse::ListServicesResponse(list)) => Ok(Answer::Services(
                        list.service.into_iter().map(|s| s.name).collect(),
                    )),
                    Some(MessageResponse::FileDescriptorResponse(files)) => {
                        Ok(Answer::Files(files.file_descriptor_proto))
                    }
                    Some(MessageResponse::ErrorResponse(error)) => {
                        Ok(Answer::Refused(error.error_message))
                    }
                    Some(MessageResponse::AllExtensionNumbersResponse(_)) | None => Err(
                        Status::internal("The server sent an unexpected reflection answer."),
                    ),
                }
            }
        }

        impl Session for $name {
            async fn ask(&mut self, ask: Ask) -> Result<Answer, Status> {
                // The services question was queued when the stream was opened.
                if !matches!(ask, Ask::Services) {
                    let sent = self.tx.send(Self::request(ask)).await;
                    sent.map_err(|_| Status::unavailable("The reflection stream is closed."))?;
                }
                self.answer().await
            }
        }
    };
}

session!(
    SessionV1,
    v1,
    "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo"
);
session!(
    SessionV1Alpha,
    v1alpha,
    "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo"
);

/// The files collected so far, within the limits.
#[derive(Default)]
struct Collected {
    files: BTreeMap<String, FileDescriptorProto>,
    bytes: usize,
    warnings: Vec<String>,
}

fn limit_error() -> Status {
    Status::resource_exhausted(format!(
        "The server describes more than {MAX_FILES} files or 16 MiB."
    ))
}

impl Collected {
    /// Adds the encoded files and returns the imports that are still missing.
    fn add(&mut self, encoded: Vec<Vec<u8>>) -> Result<Vec<String>, Status> {
        let mut missing = Vec::new();
        for bytes in encoded {
            self.bytes += bytes.len();
            let file = FileDescriptorProto::decode(bytes.as_slice())
                .map_err(|_| Status::internal("The server sent an unreadable file descriptor."))?;
            let name = file.name().to_string();
            if self.files.contains_key(&name) {
                continue;
            }
            missing.extend(file.dependency.iter().cloned());
            self.files.insert(name, file);
            if self.files.len() > MAX_FILES || self.bytes > MAX_BYTES {
                return Err(limit_error());
            }
        }
        missing.retain(|name| !self.files.contains_key(name));
        Ok(missing)
    }

    /// A well-known type the server did not send is taken from the bundled files.
    fn add_bundled(&mut self, name: &str) -> bool {
        let Some(file) = DescriptorPool::global().get_file_by_name(name) else {
            return false;
        };
        self.files
            .insert(name.to_string(), file.file_descriptor_proto().clone());
        true
    }
}

impl Collected {
    /// Keeps at most [`MAX_WARNINGS`] warnings, the text of each cut short: a server
    /// controls what they say.
    fn warn(&mut self, text: String) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings
                .push(text.chars().take(MAX_WARNING_CHARS).collect());
        } else if self.warnings.len() == MAX_WARNINGS {
            self.warnings
                .push("More warnings were left out.".to_string());
        }
    }
}

async fn collect(session: &mut impl Session) -> Result<Collected, Status> {
    let services = match session.ask(Ask::Services).await? {
        Answer::Services(names) => names,
        Answer::Refused(message) => return Err(Status::unknown(message)),
        Answer::Files(_) => return Err(Status::internal("The server did not list its services.")),
    };
    let mut collected = Collected::default();
    let mut names: Vec<String> = services
        .into_iter()
        .filter(|name| !name.starts_with(REFLECTION_PREFIX))
        .collect();
    if names.len() > MAX_SERVICES {
        collected.warn(format!(
            "The server lists {} services; only the first {MAX_SERVICES} are read.",
            names.len()
        ));
        names.truncate(MAX_SERVICES);
    }
    let mut queue: VecDeque<Ask> = names.into_iter().map(Ask::Symbol).collect();
    while let Some(ask) = queue.pop_front() {
        let label = match &ask {
            Ask::File(name) if collected.files.contains_key(name) => continue,
            Ask::File(name) if collected.add_bundled(name) => continue,
            Ask::File(name) | Ask::Symbol(name) => name.clone(),
            Ask::Services => continue,
        };
        match session.ask(ask).await? {
            Answer::Files(encoded) => {
                queue.extend(collected.add(encoded)?.into_iter().map(Ask::File));
            }
            Answer::Refused(message) => collected.warn(format!(
                "The server has no description of `{label}`: {message}"
            )),
            Answer::Services(_) => {
                return Err(Status::internal(
                    "The server sent an unexpected reflection answer.",
                ))
            }
        }
    }
    Ok(collected)
}

async fn collect_any(channel: &Channel, metadata: &MetadataMap) -> Result<Collected, Status> {
    let v1 = async {
        let mut session = SessionV1::open(channel.clone(), metadata.clone()).await?;
        collect(&mut session).await
    };
    match v1.await {
        Err(status) if status.code() == Code::Unimplemented => {
            let mut session = SessionV1Alpha::open(channel.clone(), metadata.clone()).await?;
            collect(&mut session).await
        }
        other => other,
    }
}

/// The descriptors a server reported.
#[derive(Debug, Clone)]
pub struct Reflected {
    pub pool: DescriptorPool,
    /// The encoded `FileDescriptorSet`, for the cache.
    pub encoded: Vec<u8>,
    pub files: Vec<String>,
    pub warnings: Vec<String>,
}

fn status_error(status: &Status) -> AppError {
    let message = match status.code() {
        Code::Unimplemented => "The server does not support reflection. Use proto files.".into(),
        code => format!(
            "Reflection failed: {} {}",
            code_name(code),
            status.message()
        ),
    };
    AppError::new(ErrorCode::Network, message)
}

/// Builds the pool of an encoded `FileDescriptorSet` (also what the cache holds).
pub fn pool_of(encoded: &[u8]) -> Result<(DescriptorPool, Vec<String>), AppError> {
    let unusable = |detail: String| {
        AppError::invalid_input("The server's descriptors cannot be used.").with_detail(detail)
    };
    let set = FileDescriptorSet::decode(encoded).map_err(|source| unusable(source.to_string()))?;
    let files = set
        .file
        .iter()
        .map(|file| file.name().to_string())
        .collect();
    let mut pool = DescriptorPool::new();
    pool.add_file_descriptor_protos(set.file)
        .map_err(|source| unusable(source.to_string()))?;
    Ok((pool, files))
}

/// Fetches the descriptors of every service over `channel`. Errors are masked.
pub async fn fetch(channel: &Channel, spec: &GrpcCallSpec) -> Result<Reflected, AppError> {
    let metadata = MetadataMap::from_headers(spec.metadata.clone());
    let collected = tokio::time::timeout(TIME_LIMIT, collect_any(channel, &metadata))
        .await
        .map_err(|_| AppError::new(ErrorCode::Timeout, "Reflection took longer than 30 s."))?
        .map_err(|status| super::connect::mask_error(status_error(&status), &spec.masker()))?;
    let set = FileDescriptorSet {
        file: collected.files.into_values().collect(),
    };
    let encoded = set.encode_to_vec();
    let (pool, files) = pool_of(&encoded)?;
    Ok(Reflected {
        pool,
        encoded,
        files,
        warnings: collected.warnings,
    })
}

#[cfg(test)]
#[path = "reflection_tests.rs"]
mod tests;
