//! The WSDL import walker (plan 06 §7.1, §7.3 🔒): breadth-first over `wsdl:import`,
//! `xsd:import` and `xsd:include`, de-duplicated by absolute location, with every import
//! and every redirect hop checked by [`super::rules`]. Credentials go only to the root's
//! origin. A refused import is skipped with a warning; a broken limit fails the import.

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use url::Url;

use super::model::WsdlModel;
use super::rules::{
    check_url, display_path, display_url, fetched_url, is_absolute_url, join_file, join_url,
    same_origin,
};
use super::wsdl::{imports, Definitions};
use super::xml::{decode, parse, XmlLimits, MAX_DOCUMENT_BYTES, NODES_LIMIT};
use crate::error::{AppError, ErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkLimits {
    pub document_bytes: usize,
    pub total_bytes: usize,
    pub documents: usize,
    pub depth: usize,
    pub redirects: usize,
    pub wall: Duration,
}

impl Default for WalkLimits {
    fn default() -> Self {
        Self {
            document_bytes: MAX_DOCUMENT_BYTES,
            total_bytes: 30 * 1024 * 1024,
            documents: 50,
            depth: 8,
            redirects: 5,
            wall: Duration::from_secs(60),
        }
    }
}

/// What one GET returned. A fetcher may follow redirects itself (checking every hop with
/// [`check_url`] against `root`, at most `max_redirects`) and return the final URL, or
/// return each redirect for the walker to check and follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    Document { url: Url, bytes: Vec<u8> },
    Redirect(Url),
}

/// One GET of an `http(s)` URL.
#[derive(Debug, Clone, Copy)]
pub struct Get<'a> {
    pub url: &'a Url,
    /// Whether the root request's auth and cookies may go with it (same origin as the
    /// root). A fetcher that follows redirects sends them only on hops of this origin.
    pub credentials: bool,
    pub max_bytes: usize,
    pub max_redirects: usize,
    /// The root URL redirect hops are checked against (`None`: a file root).
    pub root: Option<&'a Url>,
}

/// Network and disk access for the walker; tests use an in-memory one.
pub trait Fetch: Sync {
    /// One GET, at most `max_bytes` of body.
    fn get(&self, request: Get<'_>) -> impl Future<Output = Result<Fetched, AppError>> + Send;

    /// Reads `path`, which must stay inside `root_dir` once symlinks are resolved.
    fn read(
        &self,
        path: &Path,
        root_dir: &Path,
        max_bytes: usize,
    ) -> impl Future<Output = Result<Vec<u8>, AppError>> + Send;
}

/// Where an import starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Root {
    Url(Url),
    /// The canonical path of a file picked in the native dialog.
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Location {
    Url(Url),
    File(PathBuf),
}

struct Pending {
    location: Location,
    depth: usize,
    chameleon: Option<String>,
}

/// A finished walk: the model, every location fetched (no credentials, no absolute
/// paths) and the warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walked {
    pub model: WsdlModel,
    pub fetched: Vec<String>,
    pub warnings: Vec<String>,
}

struct Walker<'f, F: Fetch> {
    fetch: &'f F,
    limits: WalkLimits,
    root_url: Option<Url>,
    root_dir: PathBuf,
    seen: HashSet<Location>,
    fetched: Vec<String>,
    total: usize,
    definitions: Definitions,
}

fn limit(text: String) -> AppError {
    AppError::invalid_input(text)
}

/// Why a document was not read: a broken limit (or a cancel) fails the whole import;
/// anything else about an import skips it with a warning.
enum Failure {
    Fatal(AppError),
    Skip(AppError),
}

impl From<AppError> for Failure {
    fn from(error: AppError) -> Self {
        match error.code {
            ErrorCode::Cancelled | ErrorCode::Timeout => Self::Fatal(error),
            _ => Self::Skip(error),
        }
    }
}

impl<F: Fetch> Walker<'_, F> {
    fn warn(&mut self, text: String) {
        if !self.definitions.warnings.contains(&text) {
            self.definitions.warnings.push(text);
        }
    }

    fn show(&self, location: &Location) -> String {
        match location {
            Location::Url(url) => fetched_url(url),
            Location::File(path) => display_path(path, &self.root_dir),
        }
    }

    /// GETs `url`, following up to `redirects` hops, each checked like an import.
    /// Returns the final URL and the body.
    async fn get(&self, url: &Url) -> Result<(Url, Vec<u8>), AppError> {
        let mut current = url.clone();
        for hop in 0..=self.limits.redirects {
            let request = Get {
                url: &current,
                credentials: self
                    .root_url
                    .as_ref()
                    .is_some_and(|root| same_origin(root, &current)),
                max_bytes: self.limits.document_bytes,
                max_redirects: self.limits.redirects - hop,
                root: self.root_url.as_ref(),
            };
            match self.fetch.get(request).await? {
                Fetched::Document { url, bytes } => return Ok((url, bytes)),
                Fetched::Redirect(next) => {
                    check_url(&next, self.root_url.as_ref())?;
                    current = next;
                }
            }
        }
        Err(limit(format!(
            "{} redirected more than {} times.",
            display_url(url),
            self.limits.redirects
        )))
    }

    async fn load(&mut self, location: &Location) -> Result<(Location, Vec<u8>), AppError> {
        match location {
            Location::Url(url) => {
                let (final_url, bytes) = self.get(url).await?;
                Ok((Location::Url(final_url), bytes))
            }
            Location::File(path) => {
                let bytes = self
                    .fetch
                    .read(path, &self.root_dir, self.limits.document_bytes)
                    .await?;
                Ok((location.clone(), bytes))
            }
        }
    }

    /// Where an import of the document at `base` points, or why it is refused.
    fn target(&self, base: &Location, location: &str) -> Result<Location, AppError> {
        match base {
            Location::Url(base) => {
                join_url(base, location, self.root_url.as_ref()).map(Location::Url)
            }
            Location::File(_) if is_absolute_url(location) => {
                let url = Url::parse(location.trim())
                    .map_err(|_| limit(format!("Refused \"{location}\": not a valid location.")))?;
                check_url(&url, None)?;
                Ok(Location::Url(url))
            }
            Location::File(path) => join_file(path, location, &self.root_dir).map(Location::File),
        }
    }

    async fn visit(
        &mut self,
        pending: Pending,
        queue: &mut VecDeque<Pending>,
    ) -> Result<(), Failure> {
        if self.fetched.len() >= self.limits.documents {
            return Err(Failure::Fatal(limit(format!(
                "The WSDL imports more than {} documents.",
                self.limits.documents
            ))));
        }
        let (at, bytes) = self.load(&pending.location).await?;
        self.total += bytes.len();
        if self.total > self.limits.total_bytes {
            return Err(Failure::Fatal(limit(format!(
                "The WSDL and its imports are larger than {} MiB.",
                self.limits.total_bytes / (1024 * 1024)
            ))));
        }
        self.fetched.push(self.show(&at));
        self.seen.insert(at.clone());
        let text = decode(&bytes, self.limits.document_bytes)?;
        if text.contains("{{") {
            self.warn(format!(
                "{} contains \"\\{{{{\": it is escaped in generated envelopes, so it never resolves as a variable.",
                self.show(&at)
            ));
        }
        let limits = XmlLimits {
            max_bytes: self.limits.document_bytes.max(text.len()),
            nodes_limit: NODES_LIMIT,
        };
        let document = parse(&text, limits)?;
        self.definitions
            .read(&document, pending.chameleon.as_deref())?;
        for import in imports(&document) {
            let target = match self.target(&at, &import.location) {
                Ok(target) => target,
                Err(error) => {
                    self.warn(error.message);
                    continue;
                }
            };
            if !self.seen.insert(target.clone()) {
                continue;
            }
            if pending.depth + 1 > self.limits.depth {
                return Err(Failure::Fatal(limit(format!(
                    "Imports are nested deeper than {} levels.",
                    self.limits.depth
                ))));
            }
            queue.push_back(Pending {
                location: target,
                depth: pending.depth + 1,
                chameleon: import.chameleon,
            });
        }
        Ok(())
    }

    async fn run(mut self, root: Location) -> Result<Walked, AppError> {
        self.seen.insert(root.clone());
        let mut queue = VecDeque::from([Pending {
            location: root,
            depth: 0,
            chameleon: None,
        }]);
        let mut first = true;
        while let Some(pending) = queue.pop_front() {
            let shown = self.show(&pending.location);
            match self.visit(pending, &mut queue).await {
                Ok(()) => {}
                Err(Failure::Fatal(error)) => return Err(error),
                // The root itself must be read.
                Err(Failure::Skip(error)) if first => return Err(error),
                Err(Failure::Skip(error)) => {
                    self.warn(format!("Skipped {shown}: {}", error.message));
                }
            }
            first = false;
        }
        let (model, warnings) = self.definitions.resolve()?;
        Ok(Walked {
            model,
            fetched: self.fetched,
            warnings,
        })
    }
}

/// Walks the WSDL at `root` and everything it imports, within `limits`.
pub async fn walk<F: Fetch>(fetch: &F, root: Root, limits: WalkLimits) -> Result<Walked, AppError> {
    let (root_url, root_dir, location) = match root {
        Root::Url(url) => {
            check_url(&url, Some(&url))?;
            (Some(url.clone()), PathBuf::new(), Location::Url(url))
        }
        Root::File(path) => {
            let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            (None, dir, Location::File(path))
        }
    };
    let walker = Walker {
        fetch,
        limits,
        root_url,
        root_dir,
        seen: HashSet::new(),
        fetched: Vec::new(),
        total: 0,
        definitions: Definitions::default(),
    };
    tokio::time::timeout(limits.wall, walker.run(location))
        .await
        .map_err(|_| {
            AppError::new(
                ErrorCode::Timeout,
                format!(
                    "The WSDL import took longer than {} s.",
                    limits.wall.as_secs()
                ),
            )
        })?
}

#[cfg(test)]
#[path = "walk_tests.rs"]
mod tests;
