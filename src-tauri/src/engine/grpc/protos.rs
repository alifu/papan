//! `.proto` files compiled with `protox` (plan 08 §5). Every file comes through a
//! [`ProtoSource`]; the one for disk, [`ConfinedSource`], reads only below the protos root:
//! each name goes through `store::paths::resolve`, so `..`, absolute paths and symlinks
//! are refused. `google/protobuf/*` comes from protox's bundled files. The compiler sees
//! only the files loaded here ([`ConfinedResolver`]): it never touches the disk itself.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use aws_lc_rs::digest::{Context, SHA256};
use prost_reflect::DescriptorPool;
use protox::file::{File, FileResolver, GoogleFileResolver};

use crate::error::AppError;
use crate::store::fsio::{self, EntryKind};
use crate::store::paths;

pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_FILES: usize = 200;
pub const MAX_TOTAL_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_DEPTH: usize = 32;
pub const MAX_POOLS: usize = 16;
pub const PROTO_SUFFIX: &str = ".proto";
const CURRENT_DIR: &str = ".";
const PARSER_IMPORT_MESSAGE: &str = "invalid group name";
const IMPORT_MESSAGE: &str = "invalid import or group name. Imports must stay inside the protos \
                              folder (no `..`, `.` or backslashes).";

/// Where proto sources are read from. `None` = no such file.
pub trait ProtoSource {
    fn read(&self, name: &str) -> Result<Option<String>, AppError>;
}

fn outside(name: &str) -> AppError {
    AppError::invalid_input(format!(
        "`{name}` cannot be used: proto files must be inside the protos folder \
         (no `..`, absolute paths or symlinks)."
    ))
}

/// Proto files below `root`, looked up in each import path in order.
#[derive(Debug, Clone)]
pub struct ConfinedSource {
    root: PathBuf,
    /// Relative to the root; `""` is the root itself.
    import_paths: Vec<String>,
}

impl ConfinedSource {
    /// `import_paths` are relative to `root` (`.` = the root); empty means `[.]`.
    pub fn new(root: &Path, import_paths: &[String]) -> Result<Self, AppError> {
        if fsio::entry_kind(root) != Some(EntryKind::Dir) {
            return Err(AppError::invalid_input(
                "There is no protos folder yet. Import a .proto file first.",
            ));
        }
        let cleaned: Vec<String> = import_paths
            .iter()
            .map(|path| clean_import_path(path))
            .collect::<Result<_, _>>()?;
        let import_paths = if cleaned.is_empty() {
            vec![String::new()]
        } else {
            cleaned
        };
        Ok(Self {
            root: root.to_path_buf(),
            import_paths,
        })
    }

    /// The name a root-relative file has for the compiler: its path below the first
    /// import path that holds it.
    pub fn name_of(&self, rel: &str) -> Result<String, AppError> {
        let found = self.import_paths.iter().find_map(|import_path| {
            if import_path.is_empty() {
                return Some(rel);
            }
            rel.strip_prefix(import_path.as_str())?.strip_prefix('/')
        });
        found.map(str::to_string).ok_or_else(|| {
            AppError::invalid_input(format!("`{rel}` is not inside any of the import paths."))
        })
    }
}

fn clean_import_path(path: &str) -> Result<String, AppError> {
    let trimmed = path.trim().trim_end_matches('/');
    let trimmed = trimmed.strip_prefix("./").unwrap_or(trimmed);
    if trimmed == CURRENT_DIR || trimmed.is_empty() {
        return Ok(String::new());
    }
    match paths::segments(trimmed) {
        Ok(_) => Ok(trimmed.to_string()),
        Err(_) => Err(AppError::invalid_input(format!(
            "The import path `{path}` must be a folder inside the protos folder."
        ))),
    }
}

fn read_limited(path: &Path, name: &str) -> Result<String, AppError> {
    let unreadable = |source: std::io::Error| {
        AppError::new(
            crate::error::ErrorCode::Io,
            format!("`{name}` could not be read."),
        )
        .with_detail(source.kind().to_string())
    };
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(AppError::invalid_input(format!(
            "`{name}` is larger than 1 MiB."
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| AppError::invalid_input(format!("`{name}` is not valid UTF-8.")))
}

impl ProtoSource for ConfinedSource {
    fn read(&self, name: &str) -> Result<Option<String>, AppError> {
        if !name.ends_with(PROTO_SUFFIX) {
            return Err(AppError::invalid_input(format!(
                "`{name}` cannot be used: only .proto files are read."
            )));
        }
        for import_path in &self.import_paths {
            let rel = paths::join_rel(import_path, name);
            let path = paths::resolve(&self.root, &rel).map_err(|_| outside(name))?;
            if fsio::entry_kind(&path) == Some(EntryKind::File) {
                return read_limited(&path, name).map(Some);
            }
        }
        Ok(None)
    }
}

fn is_well_known(name: &str) -> bool {
    name.starts_with("google/protobuf/") && GoogleFileResolver::new().open_file(name).is_ok()
}

/// A compile error as text: `file:line:column: message` where protox knows the place.
fn compile_error(error: &protox::Error) -> AppError {
    let text = format!("{error:?}");
    // protox-parse 0.10 reports a refused import path with the group-name message.
    let text = match text.strip_suffix(PARSER_IMPORT_MESSAGE) {
        Some(place) => format!("{place}{IMPORT_MESSAGE}"),
        None => text,
    };
    AppError::invalid_input(text)
}

/// The sources of a compile: the entry files and everything they import, parsed.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    entries: Vec<String>,
    files: BTreeMap<String, File>,
}

impl Loaded {
    /// Names of the loaded files (no well-known types), sorted.
    pub fn names(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    pub fn source(&self, name: &str) -> Option<&str> {
        self.files.get(name).and_then(File::source)
    }

    fn key(&self) -> String {
        let mut context = Context::new(&SHA256);
        let mut feed = |text: &str| {
            context.update(&(text.len() as u64).to_be_bytes());
            context.update(text.as_bytes());
        };
        self.entries.iter().for_each(|entry| feed(entry));
        feed("\n");
        for (name, file) in &self.files {
            feed(name);
            feed(file.source().unwrap_or_default());
        }
        let hash = context.finish();
        hash.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Reads `entries` (compiler names) and their import closure from `source`, within the
/// limits. An import that does not exist is left to the compiler, which names the line.
pub fn load(source: &dyn ProtoSource, entries: &[String]) -> Result<Loaded, AppError> {
    let mut files = BTreeMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack: Vec<(String, usize, bool)> = entries
        .iter()
        .rev()
        .map(|name| (name.clone(), 0, true))
        .collect();
    let mut total = 0_usize;
    while let Some((name, depth, is_entry)) = stack.pop() {
        if is_well_known(&name) || !seen.insert(name.clone()) {
            continue;
        }
        if depth > MAX_DEPTH {
            return Err(AppError::invalid_input(format!(
                "Imports are nested deeper than {MAX_DEPTH} files at `{name}`."
            )));
        }
        let Some(text) = source.read(&name)? else {
            if is_entry {
                return Err(AppError::not_found(format!("`{name}` does not exist.")));
            }
            continue;
        };
        total += text.len();
        if files.len() >= MAX_FILES || total > MAX_TOTAL_BYTES {
            return Err(AppError::invalid_input(format!(
                "The proto files are too many or too large (at most {MAX_FILES} files and 10 MiB)."
            )));
        }
        let file = File::from_source(&name, &text).map_err(|error| compile_error(&error))?;
        let imports = &file.file_descriptor_proto().dependency;
        stack.extend(imports.iter().map(|dep| (dep.clone(), depth + 1, false)));
        files.insert(name, file);
    }
    Ok(Loaded {
        entries: entries.to_vec(),
        files,
    })
}

/// Serves the compiler from the loaded files and the bundled well-known types only.
pub struct ConfinedResolver {
    files: BTreeMap<String, File>,
    google: GoogleFileResolver,
}

impl FileResolver for ConfinedResolver {
    fn open_file(&self, name: &str) -> Result<File, protox::Error> {
        match self.files.get(name) {
            Some(file) => Ok(file.clone()),
            None => self.google.open_file(name),
        }
    }
}

/// A compiled set of proto files.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub pool: DescriptorPool,
    /// The files read (compiler names, no well-known types), sorted.
    pub files: Vec<String>,
}

pub fn compile(loaded: &Loaded) -> Result<Compiled, AppError> {
    let resolver = ConfinedResolver {
        files: loaded.files.clone(),
        google: GoogleFileResolver::new(),
    };
    let mut compiler = protox::Compiler::with_file_resolver(resolver);
    compiler.include_imports(true);
    for entry in &loaded.entries {
        compiler
            .open_file(entry)
            .map_err(|error| compile_error(&error))?;
    }
    Ok(Compiled {
        pool: compiler.descriptor_pool(),
        files: loaded.names(),
    })
}

/// The compiled pools of the last [`MAX_POOLS`] distinct inputs, keyed by the SHA-256 of
/// the entry names and every file's bytes.
#[derive(Debug, Default)]
pub struct ProtoCache {
    pools: Mutex<VecDeque<(String, Arc<Compiled>)>>,
}

impl ProtoCache {
    /// Compiles `protos` (relative to `root`) with `import_paths`. Blocking: file reads.
    pub fn compile(
        &self,
        root: &Path,
        import_paths: &[String],
        protos: &[String],
    ) -> Result<Arc<Compiled>, AppError> {
        if protos.is_empty() {
            return Err(AppError::invalid_input(
                "Add a .proto file, or switch to server reflection.",
            ));
        }
        let source = ConfinedSource::new(root, import_paths)?;
        let entries: Vec<String> = protos
            .iter()
            .map(|rel| source.name_of(rel))
            .collect::<Result<_, _>>()?;
        self.compile_from(&source, &entries)
    }

    pub fn compile_from(
        &self,
        source: &dyn ProtoSource,
        entries: &[String],
    ) -> Result<Arc<Compiled>, AppError> {
        let loaded = load(source, entries)?;
        let key = loaded.key();
        let cached = |pools: &VecDeque<(String, Arc<Compiled>)>| {
            let hit = pools.iter().find(|(cached, _)| *cached == key);
            hit.map(|(_, compiled)| Arc::clone(compiled))
        };
        if let Some(hit) = cached(&self.lock()) {
            return Ok(hit);
        }
        let compiled = Arc::new(compile(&loaded)?);
        let mut pools = self.lock();
        pools.push_back((key, Arc::clone(&compiled)));
        while pools.len() > MAX_POOLS {
            pools.pop_front();
        }
        Ok(compiled)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<(String, Arc<Compiled>)>> {
        self.pools.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    pub(crate) fn pool_count(&self) -> usize {
        self.lock().len()
    }
}

#[cfg(test)]
#[path = "protos_tests.rs"]
pub(crate) mod tests;
