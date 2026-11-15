//! Proto files through `grpc_schema` (plan 08 §9): the greeter fixtures with a sibling
//! import and a well-known type, and bad inputs that must fail clearly without reading
//! anything outside the protos folder: syntax errors, `..` and absolute paths, a file
//! over the size limit, an import cycle and a symlink that escapes the folder.

mod support;

use papan_lib::engine::grpc::types::GrpcMethodKind;
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::RequestContent;
use support::grpc_world::{content, fixtures, world, World};

const SYNTAX: &str = "syntax = \"proto3\";\n";
/// Found only in the file outside the protos folder: an error quoting it means it was read.
const OUTSIDE_MARKER: &str = "OutsideMarker";

fn protos(entries: &[&str]) -> RequestContent {
    let mut request = content("http://127.0.0.1:1", "", "");
    request.grpc.protos = entries.iter().map(|e| e.to_string()).collect();
    request.grpc.service.clear();
    request
}

async fn fails(w: &World, entries: &[&str]) -> AppError {
    let error = w.schema(&protos(entries)).await.unwrap_err();
    let shown = format!("{error:?}");
    assert!(
        !shown.contains(OUTSIDE_MARKER),
        "read outside the root: {shown}"
    );
    assert_ne!(error.code, ErrorCode::Internal, "{shown}");
    error
}

fn write(w: &World, rel: &str, text: &str) {
    let path = w.protos_root().join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A proto next to (not inside) the protos folder.
fn outside(w: &World) -> std::path::PathBuf {
    let path = w.protos_root().parent().unwrap().join("outside.proto");
    std::fs::write(
        &path,
        format!("{SYNTAX}message {OUTSIDE_MARKER} {{ string x = ; }}\n"),
    )
    .unwrap();
    path
}

#[tokio::test]
async fn the_greeter_compiles_with_its_sibling_import_and_a_well_known_type() {
    let w = world();

    let schema = w
        .schema(&protos(&["greeter/v1/greeter.proto"]))
        .await
        .unwrap();

    let greeter = &schema.services[0];
    assert_eq!(greeter.full_name, "greeter.v1.Greeter");
    let kinds: Vec<(&str, GrpcMethodKind)> = greeter
        .methods
        .iter()
        .map(|m| (m.name.as_str(), m.kind))
        .collect();
    assert!(
        kinds.contains(&("SayHello", GrpcMethodKind::Unary)),
        "{kinds:?}"
    );
    assert!(kinds.contains(&("Ticks", GrpcMethodKind::ServerStreaming)));
    assert!(kinds.contains(&("Collect", GrpcMethodKind::ClientStreaming)));
    assert!(kinds.contains(&("Chat", GrpcMethodKind::Bidi)));
    assert!(
        schema
            .files
            .contains(&"greeter/v1/messages.proto".to_string()),
        "{:?}",
        schema.files
    );
    let hello = greeter
        .methods
        .iter()
        .find(|m| m.name == "SayHello")
        .unwrap();
    assert!(hello.template.contains("\"name\""), "{}", hello.template);
}

#[tokio::test]
async fn a_syntax_error_names_the_file_and_line() {
    let w = world();

    let error = fails(&w, &["bad-syntax.proto"]).await;

    let shown = format!("{} {}", error.message, error.detail.unwrap_or_default());
    assert!(shown.contains("bad-syntax.proto"), "{shown}");
    assert!(shown.contains(":6"), "no line in {shown}");
}

#[tokio::test]
async fn entry_paths_with_dot_dot_or_a_root_are_refused() {
    let w = world();
    let outside = outside(&w);

    fails(&w, &["../outside.proto"]).await;
    fails(&w, &[outside.to_str().unwrap()]).await;
    let yaml = std::fs::read_to_string(fixtures().join("grpc_files/bad-proto-escape.http.yaml"));
    assert!(
        yaml.unwrap().contains("../greeter"),
        "the fixture still escapes"
    );
}

#[tokio::test]
async fn imports_with_dot_dot_or_an_absolute_path_are_refused() {
    let w = world();
    let outside = outside(&w);
    write(
        &w,
        "up.proto",
        &format!("{SYNTAX}import \"../outside.proto\";\n"),
    );
    let absolute = format!("{SYNTAX}import \"{}\";\n", outside.display());
    write(&w, "abs.proto", &absolute);

    let up = fails(&w, &["up.proto"]).await;
    fails(&w, &["abs.proto"]).await;

    assert!(up.message.contains("inside the protos"), "{up:?}");
}

#[tokio::test]
async fn a_file_over_the_size_limit_is_refused() {
    let w = world();
    let padding = "// padding\n".repeat(2 * 1024 * 1024 / 11 + 1);
    write(
        &w,
        "big.proto",
        &format!("{SYNTAX}{padding}message Big {{}}\n"),
    );

    let error = fails(&w, &["big.proto"]).await;

    assert!(error.message.contains("MiB"), "{error:?}");
}

#[tokio::test]
async fn an_import_cycle_is_an_error() {
    let w = world();
    write(
        &w,
        "cycle/a.proto",
        &format!("{SYNTAX}import \"cycle/b.proto\";\nmessage A {{}}\n"),
    );
    write(
        &w,
        "cycle/b.proto",
        &format!("{SYNTAX}import \"cycle/a.proto\";\nmessage B {{}}\n"),
    );

    let error = fails(&w, &["cycle/a.proto"]).await;

    let shown = format!("{} {}", error.message, error.detail.unwrap_or_default());
    assert!(shown.contains("cycle"), "{shown}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_that_escapes_the_protos_folder_is_refused() {
    let w = world();
    let outside = outside(&w);
    std::os::unix::fs::symlink(&outside, w.protos_root().join("link.proto")).unwrap();
    write(
        &w,
        "via.proto",
        &format!("{SYNTAX}import \"link.proto\";\n"),
    );

    fails(&w, &["link.proto"]).await;
    fails(&w, &["via.proto"]).await;
}
