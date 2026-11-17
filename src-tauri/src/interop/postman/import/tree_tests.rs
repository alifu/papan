use serde_json::{json, Value};

use super::*;
use crate::interop::postman::types::ReportLevel;
use crate::store::types::Auth;

fn plan(value: &Value) -> (PlannedCollection, Report) {
    let parsed = schema::parse_collection(value).unwrap();
    let mut report = Report::new();
    let planned = plan_collection("c.json", &parsed.value, &mut report).unwrap();
    (planned, report)
}

fn collection(items: Value) -> Value {
    json!({
        "info": {"name": "Shop", "_postman_id": "abc", "description": "Docs",
                 "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "item": items
    })
}

fn request(name: &str) -> Value {
    json!({"name": name, "request": {"method": "GET", "url": "https://x"}})
}

fn names(folder: &PlannedFolder) -> Vec<String> {
    entries(&folder.children)
}

#[test]
fn hostile_names_become_safe_unique_slugs_in_order() {
    let items = json!([
        request("../../etc"), request(".."), request("a/b"), request("CON"),
        request("nul\u{0}x"), request("Get"), request("get"), request(""),
        {"name": "Folder", "item": [request("x")]}
    ]);

    let (planned, _) = plan(&collection(items));

    let entries = names(&planned.root);
    assert_eq!(
        entries,
        [
            "etc.http.yaml",
            "untitled.http.yaml",
            "a-b.http.yaml",
            "con-2.http.yaml",
            "nulx.http.yaml",
            "get.http.yaml",
            "get-2.http.yaml",
            "untitled-request.http.yaml",
            "folder"
        ]
    );
    assert!(entries.iter().all(|entry| paths::is_valid_segment(entry)));
    assert_eq!(planned.root.file.order, entries);
    let PlannedNode::Request { file, .. } = &planned.root.children[7] else {
        panic!()
    };
    assert_eq!(file.name, "Untitled request");
    assert_eq!((planned.requests, planned.folders), (9, 1));
}

#[test]
fn deep_folders_are_skipped_and_reported() {
    let mut items = json!([request("leaf")]);
    for level in 0..20 {
        items = json!([{"name": format!("f{level}"), "item": items}]);
    }

    let (planned, report) = plan(&collection(items));

    assert_eq!(planned.folders as usize, MAX_IMPORT_FOLDER_DEPTH);
    assert_eq!(planned.requests, 0);
    assert!(report
        .items()
        .iter()
        .any(|item| item.level == ReportLevel::Skipped));
}

#[test]
fn too_many_items_is_a_file_error() {
    let items: Vec<Value> = (0..MAX_ITEMS + 1)
        .map(|n| request(&format!("r{n}")))
        .collect();
    let parsed = schema::parse_collection(&collection(Value::Array(items))).unwrap();

    let error = plan_collection("big.json", &parsed.value, &mut Report::new()).unwrap_err();

    assert!(error.message.starts_with("big.json: "), "{}", error.message);
}

#[test]
fn auth_is_inherited_from_folder_then_collection_and_literals_are_lifted() {
    let mut value = collection(json!([
        {"name": "F", "auth": {"type": "basic", "basic": [
            {"key": "username", "value": "u"}, {"key": "password", "value": "pw"}]},
         "item": [request("In folder")]},
        request("Top")
    ]));
    value["auth"] = json!({"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}"}]});
    value["variable"] = json!([{"key": "token", "value": "t", "type": "secret"}]);

    let (planned, report) = plan(&value);

    let PlannedNode::Folder(folder) = &planned.root.children[0] else {
        panic!()
    };
    let PlannedNode::Request { file, .. } = &folder.children[0] else {
        panic!()
    };
    assert_eq!(
        file.content().unwrap().auth,
        Auth::Basic {
            username: "u".into(),
            password: "{{in_folder_password}}".into()
        }
    );
    let PlannedNode::Request { file, .. } = &planned.root.children[1] else {
        panic!()
    };
    assert_eq!(
        file.content().unwrap().auth,
        Auth::Bearer {
            token: "{{token}}".into()
        }
    );
    assert!(planned.vars.table["in_folder_password"].secret);
    assert_eq!(planned.vars.secrets.get("in_folder_password"), Some("pw"));
    assert_eq!(planned.vars.secrets.get("token"), Some("t"));
    let messages: Vec<&str> = report.items().iter().map(|i| i.message.as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("inherit auth from folder `F`")),
        "{messages:?}"
    );
    assert!(messages.iter().all(|m| !m.contains("pw")));
}

#[test]
fn scripts_are_kept_verbatim_switched_off_and_scanned() {
    let mut value = collection(json!([{
        "name": "R", "request": {"method": "GET", "url": "https://x"},
        "event": [
            {"listen": "prerequest", "script": {"exec": ["pm.cookies.get('a');"]}},
            {"listen": "test", "script": {"exec": ["pm.test('t', () => {});", "pm.sendRequest('u');"]}}
        ]
    }]));
    value["event"] = json!([{"listen": "prerequest", "script": {"exec": ["console.log(1);"]}}]);

    let (planned, report) = plan(&value);

    let PlannedNode::Request { file, .. } = &planned.root.children[0] else {
        panic!()
    };
    let scripts = file.content().unwrap().scripts;
    assert_eq!(scripts.pre_request, "pm.cookies.get('a');");
    assert_eq!(
        scripts.post_response,
        "pm.test('t', () => {});\npm.sendRequest('u');"
    );
    assert_eq!(file.extra["scripts"]["enabled"], json!(false));
    assert_eq!(planned.file.extra["scripts"]["enabled"], json!(false));
    assert_eq!(planned.scripts, 3);
    let warnings: Vec<&str> = report
        .items()
        .iter()
        .filter(|i| i.level == ReportLevel::Warning)
        .map(|i| i.message.as_str())
        .collect();
    assert!(warnings
        .iter()
        .any(|m| m.contains("pm.cookies") && m.contains("line 1")));
    assert!(report
        .items()
        .iter()
        .any(|i| i.message.contains("pm.sendRequest")));
}

#[test]
fn collection_file_keeps_postman_id_and_description() {
    let (planned, _) = plan(&collection(json!([])));

    let text = planned.file.to_yaml().unwrap();

    assert_eq!(planned.name, "Shop");
    assert!(text.contains("postman_id: abc"), "{text}");
    assert!(text.contains("description: Docs"), "{text}");
}

#[test]
fn invalid_requests_are_skipped_with_their_path() {
    let huge = "u".repeat(10_000);
    let items = json!([
        {"name": "Long", "request": {"method": "GET", "url": huge}},
        {"name": "Bad method", "request": {"method": "G E T", "url": "https://x"}},
        request("Ok")
    ]);

    let (planned, report) = plan(&collection(items));

    assert_eq!(names(&planned.root), ["ok.http.yaml"]);
    let skipped: Vec<&str> = report
        .items()
        .iter()
        .filter(|i| i.level == ReportLevel::Skipped)
        .map(|i| i.path.as_str())
        .collect();
    assert_eq!(skipped, ["item[0]", "item[1]"]);
}
