use std::collections::HashMap;

use super::*;

/// An in-memory table of proto files.
pub(crate) struct Table(pub HashMap<String, String>);

impl Table {
    pub(crate) fn of(files: &[(&str, &str)]) -> Self {
        Self(
            files
                .iter()
                .map(|(name, text)| (name.to_string(), text.to_string()))
                .collect(),
        )
    }
}

impl ProtoSource for Table {
    fn read(&self, name: &str) -> Result<Option<String>, AppError> {
        Ok(self.0.get(name).cloned())
    }
}

pub(crate) fn pool_of(files: &[(&str, &str)], entry: &str) -> DescriptorPool {
    let loaded = load(&Table::of(files), &[entry.to_string()]).unwrap();
    compile(&loaded).unwrap().pool
}

const GREETER: &str = r#"syntax = "proto3";
package greeter.v1;
import "common/types.proto";
import "google/protobuf/timestamp.proto";
service Greeter { rpc SayHello (common.Name) returns (Reply); }
message Reply { string text = 1; google.protobuf.Timestamp at = 2; }
"#;
const TYPES: &str = "syntax = \"proto3\";\npackage common;\nmessage Name { string name = 1; }\n";

fn entries(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

fn disk(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn message_of(result: Result<Arc<Compiled>, AppError>) -> String {
    result.unwrap_err().message
}

#[test]
fn a_relative_import_and_a_well_known_type_compile() {
    let table = Table::of(&[("greeter.proto", GREETER), ("common/types.proto", TYPES)]);

    let compiled = compile(&load(&table, &entries(&["greeter.proto"])).unwrap()).unwrap();

    assert_eq!(compiled.files, ["common/types.proto", "greeter.proto"]);
    let service = compiled
        .pool
        .get_service_by_name("greeter.v1.Greeter")
        .unwrap();
    assert_eq!(service.methods().next().unwrap().name(), "SayHello");
}

#[test]
fn a_syntax_error_names_file_line_and_column() {
    let table = Table::of(&[("bad.proto", "syntax = \"proto3\";\nmessage { }\n")]);

    let error = load(&table, &entries(&["bad.proto"])).unwrap_err();

    assert!(error.message.starts_with("bad.proto:2:9: "), "{error}");
}

#[test]
fn a_missing_import_names_the_importing_line() {
    let table = Table::of(&[("greeter.proto", GREETER)]);

    let error = compile(&load(&table, &entries(&["greeter.proto"])).unwrap()).unwrap_err();

    assert!(error.message.starts_with("greeter.proto:3:1: "), "{error}");
    assert!(error.message.contains("common/types.proto"), "{error}");
}

#[test]
fn a_missing_entry_file_and_an_import_cycle_are_errors() {
    let missing = load(&Table::of(&[]), &entries(&["nope.proto"])).unwrap_err();
    assert_eq!(missing.code, crate::error::ErrorCode::NotFound);

    let cycle = Table::of(&[
        ("a.proto", "syntax = \"proto3\";\nimport \"b.proto\";\n"),
        ("b.proto", "syntax = \"proto3\";\nimport \"a.proto\";\n"),
    ]);
    let error = compile(&load(&cycle, &entries(&["a.proto"])).unwrap()).unwrap_err();
    assert!(error.message.contains("cycle"), "{error}");
}

#[test]
fn the_file_count_and_depth_limits_hold() {
    let chain = |count: usize| -> Vec<(String, String)> {
        (0..count)
            .map(|n| {
                let import = format!("import \"f{}.proto\";\n", n + 1);
                let import = if n + 1 < count { import } else { String::new() };
                (
                    format!("f{n}.proto"),
                    format!("syntax = \"proto3\";\n{import}"),
                )
            })
            .collect()
    };
    let deep = chain(MAX_DEPTH + 2);
    let deep = Table(deep.into_iter().collect());
    let error = load(&deep, &entries(&["f0.proto"])).unwrap_err();
    assert!(error.message.contains("nested deeper"), "{error}");

    let many: HashMap<String, String> = (0..=MAX_FILES)
        .map(|n| (format!("m{n}.proto"), "syntax = \"proto3\";\n".to_string()))
        .collect();
    let names: Vec<String> = many.keys().cloned().collect();
    let error = load(&Table(many), &names).unwrap_err();
    assert!(error.message.contains("too many"), "{error}");
}

#[test]
fn the_total_size_limit_holds() {
    let filler = format!(
        "syntax = \"proto3\";\n//{}\n",
        "x".repeat(MAX_FILE_BYTES - 64)
    );
    let many: HashMap<String, String> = (0..11)
        .map(|n| (format!("big{n}.proto"), filler.clone()))
        .collect();
    let names: Vec<String> = many.keys().cloned().collect();

    let error = load(&Table(many), &names).unwrap_err();

    assert!(error.message.contains("too large"), "{error}");
}

#[test]
fn files_are_found_through_the_import_paths_in_order() {
    let dir = disk(&[
        ("greeter/greeter.proto", GREETER),
        ("third_party/common/types.proto", TYPES),
    ]);
    let cache = ProtoCache::default();
    let paths = entries(&[".", "third_party"]);

    let compiled = cache
        .compile(dir.path(), &paths, &entries(&["greeter/greeter.proto"]))
        .unwrap();

    assert_eq!(
        compiled.files,
        ["common/types.proto", "greeter/greeter.proto"]
    );
    let source = ConfinedSource::new(dir.path(), &paths).unwrap();
    assert_eq!(
        source.name_of("third_party/common/types.proto").unwrap(),
        "third_party/common/types.proto"
    );
    let only_third = ConfinedSource::new(dir.path(), &entries(&["third_party/"])).unwrap();
    assert_eq!(
        only_third
            .name_of("third_party/common/types.proto")
            .unwrap(),
        "common/types.proto"
    );
    assert!(only_third.name_of("greeter/greeter.proto").is_err());
}

#[test]
fn imports_that_leave_the_root_are_refused() {
    let outer = disk(&[("secret.proto", TYPES), ("root/ok.proto", TYPES)]);
    let root = outer.path().join("root");
    let cache = ProtoCache::default();
    // Forward slashes: a Windows backslash would be read as a string escape in the .proto.
    let absolute = outer
        .path()
        .join("secret.proto")
        .to_str()
        .unwrap()
        .replace('\\', "/");
    for import in ["../secret.proto", absolute.as_str()] {
        let text = format!("syntax = \"proto3\";\nimport \"{import}\";\n");
        std::fs::write(root.join("main.proto"), text).unwrap();

        let message = message_of(cache.compile(&root, &[], &entries(&["main.proto"])));

        assert!(message.contains("inside the protos folder"), "{message}");
    }
    for bad in ["../secret.proto", "/etc/hosts.proto"] {
        assert!(cache.compile(&root, &[], &entries(&[bad])).is_err());
    }
    let bad_path = cache.compile(&root, &entries(&["../"]), &entries(&["ok.proto"]));
    assert!(message_of(bad_path).contains("import path"));
}

#[cfg(unix)]
#[test]
fn symlinks_are_refused_for_files_folders_and_the_root() {
    let outer = disk(&[
        ("secret.proto", TYPES),
        ("elsewhere/x.proto", TYPES),
        (
            "root/main.proto",
            "syntax = \"proto3\";\nimport \"link.proto\";\n",
        ),
    ]);
    let root = outer.path().join("root");
    std::os::unix::fs::symlink(outer.path().join("secret.proto"), root.join("link.proto")).unwrap();
    std::os::unix::fs::symlink(outer.path().join("elsewhere"), root.join("dir")).unwrap();
    std::os::unix::fs::symlink(&root, outer.path().join("rootlink")).unwrap();
    let cache = ProtoCache::default();

    let file = message_of(cache.compile(&root, &[], &entries(&["main.proto"])));
    let folder = message_of(cache.compile(&root, &[], &entries(&["dir/x.proto"])));
    let linked_root = cache.compile(
        &outer.path().join("rootlink"),
        &[],
        &entries(&["main.proto"]),
    );

    assert!(file.contains("symlinks"), "{file}");
    assert!(folder.contains("symlinks"), "{folder}");
    assert!(message_of(linked_root).contains("no protos folder"));
}

#[test]
fn only_small_utf8_proto_files_are_read() {
    let dir = disk(&[
        ("notes.txt", TYPES),
        ("big.proto", &"x".repeat(MAX_FILE_BYTES + 1)),
    ]);
    std::fs::write(dir.path().join("binary.proto"), [0xff, 0xfe, 0x00]).unwrap();
    let cache = ProtoCache::default();
    let run = |name: &str| message_of(cache.compile(dir.path(), &[], &entries(&[name])));

    assert!(run("notes.txt").contains("only .proto files"));
    assert!(run("big.proto").contains("larger than 1 MiB"));
    assert!(run("binary.proto").contains("not valid UTF-8"));
    assert!(message_of(cache.compile(dir.path(), &[], &[])).contains("Add a .proto file"));
    let missing = cache.compile(&dir.path().join("none"), &[], &entries(&["a.proto"]));
    assert!(message_of(missing).contains("no protos folder"));
}

#[test]
fn equal_inputs_share_a_pool_and_the_cache_is_bounded() {
    let cache = ProtoCache::default();
    let table = Table::of(&[("types.proto", TYPES)]);

    let first = cache
        .compile_from(&table, &entries(&["types.proto"]))
        .unwrap();
    let second = cache
        .compile_from(&table, &entries(&["types.proto"]))
        .unwrap();
    assert!(Arc::ptr_eq(&first, &second));

    for n in 0..MAX_POOLS + 3 {
        let text = format!("syntax = \"proto3\";\nmessage M{n} {{}}\n");
        let table = Table::of(&[("m.proto", &text)]);
        cache.compile_from(&table, &entries(&["m.proto"])).unwrap();
    }
    assert_eq!(cache.pool_count(), MAX_POOLS);
    let again = cache
        .compile_from(&table, &entries(&["types.proto"]))
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &again));
}
