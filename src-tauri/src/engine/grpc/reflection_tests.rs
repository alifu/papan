use std::collections::HashMap;

use super::*;

/// A reflection server as a table: symbol → file, and the files by name.
#[derive(Default)]
struct Fake {
    services: Vec<String>,
    symbols: HashMap<String, String>,
    files: HashMap<String, FileDescriptorProto>,
    asked: Vec<String>,
}

fn file(name: &str, deps: &[&str]) -> FileDescriptorProto {
    FileDescriptorProto {
        name: Some(name.to_string()),
        dependency: deps.iter().map(|dep| dep.to_string()).collect(),
        syntax: Some("proto3".into()),
        ..FileDescriptorProto::default()
    }
}

impl Fake {
    fn with(mut self, service: &str, files: Vec<FileDescriptorProto>) -> Self {
        let first = files[0].name().to_string();
        self.services.push(service.to_string());
        self.symbols.insert(service.to_string(), first);
        for file in files {
            self.files.insert(file.name().to_string(), file);
        }
        self
    }

    fn encoded(&self, name: &str) -> Answer {
        match self.files.get(name) {
            Some(file) => Answer::Files(vec![file.encode_to_vec()]),
            None => Answer::Refused(format!("{name} not found")),
        }
    }
}

impl Session for Fake {
    async fn ask(&mut self, ask: Ask) -> Result<Answer, Status> {
        Ok(match ask {
            Ask::Services => Answer::Services(self.services.clone()),
            Ask::Symbol(symbol) => {
                self.asked.push(format!("symbol:{symbol}"));
                let name = self.symbols.get(&symbol).cloned().unwrap_or_default();
                self.encoded(&name)
            }
            Ask::File(name) => {
                self.asked.push(format!("file:{name}"));
                self.encoded(&name)
            }
        })
    }
}

#[tokio::test]
async fn imports_are_fetched_breadth_first_and_each_file_once() {
    let mut fake = Fake::default()
        .with(
            "a.Svc",
            vec![
                file("a.proto", &["shared.proto", "google/protobuf/empty.proto"]),
                file("shared.proto", &["deep.proto"]),
                file("deep.proto", &[]),
            ],
        )
        .with("b.Svc", vec![file("b.proto", &["shared.proto"])])
        .with(
            "grpc.reflection.v1.ServerReflection",
            vec![file("r.proto", &[])],
        );

    let collected = collect(&mut fake).await.unwrap();

    let names: Vec<&str> = collected.files.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "a.proto",
            "b.proto",
            "deep.proto",
            "google/protobuf/empty.proto",
            "shared.proto"
        ]
    );
    assert_eq!(
        fake.asked,
        [
            "symbol:a.Svc",
            "symbol:b.Svc",
            "file:shared.proto",
            "file:deep.proto"
        ]
    );
    assert!(collected.warnings.is_empty());
}

#[tokio::test]
async fn a_file_the_server_does_not_have_is_a_warning() {
    let mut fake = Fake::default().with("a.Svc", vec![file("a.proto", &["gone.proto"])]);

    let collected = collect(&mut fake).await.unwrap();

    assert_eq!(collected.warnings.len(), 1);
    assert!(collected.warnings[0].contains("`gone.proto`"));
}

#[tokio::test]
async fn more_files_than_the_limit_is_an_error() {
    let mut files = vec![file("f0.proto", &["f1.proto"])];
    files.extend((1..=MAX_FILES).map(|n| {
        let next = format!("f{}.proto", n + 1);
        let deps: Vec<&str> = if n < MAX_FILES { vec![&next] } else { vec![] };
        file(&format!("f{n}.proto"), &deps)
    }));
    let mut fake = Fake::default().with("a.Svc", files);

    let status = collect(&mut fake).await.err().unwrap();

    assert_eq!(status.code(), Code::ResourceExhausted);
}

#[test]
fn the_byte_limit_holds_and_broken_descriptors_are_refused() {
    let mut collected = Collected {
        bytes: MAX_BYTES,
        ..Collected::default()
    };
    let over = collected.add(vec![file("a.proto", &[]).encode_to_vec()]);
    assert_eq!(over.unwrap_err().code(), Code::ResourceExhausted);

    let broken = Collected::default().add(vec![vec![0xff, 0xff, 0xff]]);
    assert_eq!(broken.unwrap_err().code(), Code::Internal);
}

#[test]
fn an_encoded_set_becomes_a_pool_or_a_clear_error() {
    let set = FileDescriptorSet {
        file: vec![file("b.proto", &["a.proto"]), file("a.proto", &[])],
    };

    let (pool, files) = pool_of(&set.encode_to_vec()).unwrap();

    assert_eq!(files, ["b.proto", "a.proto"]);
    assert!(pool.get_file_by_name("a.proto").is_some());
    let missing = FileDescriptorSet {
        file: vec![file("b.proto", &["nope.proto"])],
    };
    let error = pool_of(&missing.encode_to_vec()).unwrap_err();
    assert_eq!(error.message, "The server's descriptors cannot be used.");
    assert!(pool_of(&[0xff, 0xff]).is_err());
}

#[test]
fn statuses_become_readable_errors() {
    let unimplemented = status_error(&Status::unimplemented("x"));
    let denied = status_error(&Status::unauthenticated("bad token"));

    assert!(unimplemented
        .message
        .contains("does not support reflection"));
    assert_eq!(
        denied.message,
        "Reflection failed: UNAUTHENTICATED bad token"
    );
}

#[tokio::test]
async fn a_long_service_list_and_many_long_warnings_are_capped() {
    let mut fake = Fake::default();
    for n in 0..(MAX_SERVICES + 50) {
        // No file behind the symbol: every one is refused with a long message.
        fake.services.push(format!("svc.S{n}{}", "x".repeat(1000)));
    }

    let collected = collect(&mut fake).await.unwrap();

    let asked = fake.asked.len();
    assert_eq!(asked, MAX_SERVICES);
    assert_eq!(collected.warnings.len(), MAX_WARNINGS + 1);
    assert!(collected
        .warnings
        .iter()
        .all(|text| text.chars().count() <= MAX_WARNING_CHARS));
}
