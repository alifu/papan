//! Reads Postman JSON (v2.0 and v2.1 collections, environments, globals) into the schema types.

use serde_json::Value;

use super::parse_parts::{auth, body};
use super::read::{fail, index, scalar, Obj, Reader, Res};
use super::{
    Collection, EnvValue, Event, Folder, Header, Info, Item, Parsed, QueryParam, Request,
    RequestItem, Script, Url, Variable, VariableFile,
};

const COLLECTION_KEYS: &[&str] = &[
    "info",
    "item",
    "auth",
    "event",
    "variable",
    "protocolProfileBehavior",
];
const INFO_KEYS: &[&str] = &[
    "_postman_id",
    "name",
    "description",
    "schema",
    "version",
    "_exporter_id",
    "_collection_link",
];
const FOLDER_KEYS: &[&str] = &[
    "name",
    "description",
    "item",
    "auth",
    "event",
    "variable",
    "id",
    "_postman_id",
    "uid",
    "_postman_isSubFolder",
    "protocolProfileBehavior",
];
const REQUEST_ITEM_KEYS: &[&str] = &[
    "name",
    "description",
    "request",
    "event",
    "response",
    "id",
    "_postman_id",
    "uid",
    "protocolProfileBehavior",
    "variable",
];
const REQUEST_KEYS: &[&str] = &[
    "auth",
    "method",
    "header",
    "body",
    "url",
    "description",
    "proxy",
    "certificate",
];
const URL_KEYS: &[&str] = &[
    "raw", "protocol", "host", "port", "path", "query", "hash", "variable",
];
const PARAM_KEYS: &[&str] = &["key", "value", "disabled", "description", "type", "name"];
const EVENT_KEYS: &[&str] = &["listen", "script", "disabled", "id"];
const SCRIPT_KEYS: &[&str] = &["type", "exec", "id", "src", "name", "packages"];
const VARIABLE_KEYS: &[&str] = &[
    "key",
    "id",
    "value",
    "type",
    "disabled",
    "description",
    "name",
    "system",
];
const FILE_KEYS: &[&str] = &[
    "id",
    "name",
    "values",
    "_postman_variable_scope",
    "_postman_exported_at",
    "_postman_exported_using",
    "isPublic",
    "owner",
];
const ENV_VALUE_KEYS: &[&str] = &["key", "value", "type", "enabled", "description"];

/// Reads a v2.0 or v2.1 collection (detection of the version reads `info.schema`).
pub fn parse_collection(v: &Value) -> Result<Parsed<Collection>, super::SchemaError> {
    let mut r = Reader::default();
    let value = collection(&mut r, v)?;
    Ok(Parsed {
        value,
        unknown_fields: r.unknown,
    })
}

/// Reads an environment or globals file.
pub fn parse_variable_file(v: &Value) -> Result<Parsed<VariableFile>, super::SchemaError> {
    let mut r = Reader::default();
    let o = r.object(v, "", FILE_KEYS)?;
    let values = list(&mut r, &o, "values", env_value)?;
    let value = VariableFile {
        id: o.opt_str("id")?,
        name: o.str_or_empty("name")?,
        values,
        scope: o.opt_str("_postman_variable_scope")?,
    };
    Ok(Parsed {
        value,
        unknown_fields: r.unknown,
    })
}

fn collection(r: &mut Reader, v: &Value) -> Res<Collection> {
    let o = r.object(v, "", COLLECTION_KEYS)?;
    let Some(info_value) = o.get("info") else {
        return fail("info", "an object");
    };
    let info_obj = r.object(info_value, "info", INFO_KEYS)?;
    let info = Info {
        postman_id: info_obj.opt_str("_postman_id")?,
        name: info_obj.str_or_empty("name")?,
        description: info_obj.description("description")?,
        schema: info_obj.str_or_empty("schema")?,
    };
    Ok(Collection {
        info,
        item: list(r, &o, "item", item)?,
        auth: opt(r, &o, "auth", auth)?,
        event: list(r, &o, "event", event)?,
        variable: list(r, &o, "variable", variable)?,
        protocol_profile_behavior: o.opt_object("protocolProfileBehavior")?.cloned(),
    })
}

/// Reads every element of the array at `key` with `f`.
pub(super) fn list<T>(
    r: &mut Reader,
    o: &Obj<'_>,
    key: &str,
    f: fn(&mut Reader, &Value, &str) -> Res<T>,
) -> Res<Vec<T>> {
    let path = o.path(key);
    o.array(key)?
        .iter()
        .enumerate()
        .map(|(i, v)| f(r, v, &index(&path, i)))
        .collect()
}

pub(super) fn opt<T>(
    r: &mut Reader,
    o: &Obj<'_>,
    key: &str,
    f: fn(&mut Reader, &Value, &str) -> Res<T>,
) -> Res<Option<T>> {
    o.get(key).map(|v| f(r, v, &o.path(key))).transpose()
}

fn item(r: &mut Reader, v: &Value, path: &str) -> Res<Item> {
    let Value::Object(map) = v else {
        return fail(path, "an object");
    };
    if map.get("item").is_some_and(|i| !i.is_null()) {
        let o = r.object(v, path, FOLDER_KEYS)?;
        return Ok(Item::Folder(Folder {
            name: o.str_or_empty("name")?,
            description: o.description("description")?,
            item: list(r, &o, "item", item)?,
            auth: opt(r, &o, "auth", auth)?,
            event: list(r, &o, "event", event)?,
            variable: list(r, &o, "variable", variable)?,
        }));
    }
    let o = r.object(v, path, REQUEST_ITEM_KEYS)?;
    let Some(req) = o.get("request") else {
        return fail(path, "a request (with `request`) or a folder (with `item`)");
    };
    let mut request = request(r, req, &o.path("request"))?;
    if request.description.is_none() {
        request.description = o.description("description")?;
    }
    Ok(Item::Request(Box::new(RequestItem {
        name: o.str_or_empty("name")?,
        event: list(r, &o, "event", event)?,
        protocol_profile_behavior: o.opt_object("protocolProfileBehavior")?.cloned(),
        request,
        response_count: u32::try_from(o.array("response")?.len()).unwrap_or(u32::MAX),
    })))
}

fn request(r: &mut Reader, v: &Value, path: &str) -> Res<Request> {
    if let Value::String(raw) = v {
        let url = Url {
            raw: Some(raw.clone()),
            ..Url::default()
        };
        return Ok(Request {
            url: Some(url),
            ..Request::default()
        });
    }
    let o = r.object(v, path, REQUEST_KEYS)?;
    let header = match o.get("header") {
        Some(Value::String(text)) => header_lines(text),
        _ => list(r, &o, "header", header)?,
    };
    Ok(Request {
        auth: opt(r, &o, "auth", auth)?,
        method: o.opt_str("method")?,
        header,
        body: opt(r, &o, "body", body)?,
        url: opt(r, &o, "url", url)?,
        description: o.description("description")?,
    })
}

fn header_lines(text: &str) -> Vec<Header> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let (key, value) = line.split_once(':').unwrap_or((line, ""));
            Header {
                key: key.trim().to_string(),
                value: value.trim().to_string(),
                ..Header::default()
            }
        })
        .collect()
}

fn url(r: &mut Reader, v: &Value, path: &str) -> Res<Url> {
    if let Value::String(raw) = v {
        return Ok(Url {
            raw: Some(raw.clone()),
            ..Url::default()
        });
    }
    let o = r.object(v, path, URL_KEYS)?;
    Ok(Url {
        raw: o.opt_str("raw")?,
        protocol: o.opt_str("protocol")?,
        host: segments(&o, "host")?,
        port: o.opt_scalar("port")?,
        path: segments(&o, "path")?,
        query: list(r, &o, "query", query_param)?,
        hash: o.opt_str("hash")?,
        variable: list(r, &o, "variable", variable)?,
    })
}

/// `host` / `path`: a string or an array of strings or `{type, value}` objects.
fn segments(o: &Obj<'_>, key: &str) -> Res<Vec<String>> {
    let path = o.path(key);
    match o.get(key) {
        None => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![s.clone()]),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, seg)| match seg {
                Value::Object(m) => {
                    scalar(m.get("value").unwrap_or(&Value::Null), &index(&path, i))
                }
                other => scalar(other, &index(&path, i)),
            })
            .collect(),
        Some(_) => fail(&path, "a string or an array"),
    }
}

fn query_param(r: &mut Reader, v: &Value, path: &str) -> Res<QueryParam> {
    let o = r.object(v, path, PARAM_KEYS)?;
    Ok(QueryParam {
        key: o.opt_str("key")?,
        value: o.opt_scalar("value")?,
        disabled: o.flag("disabled", false)?,
        description: o.description("description")?,
    })
}

fn header(r: &mut Reader, v: &Value, path: &str) -> Res<Header> {
    let o = r.object(v, path, PARAM_KEYS)?;
    Ok(Header {
        key: o.str_or_empty("key")?,
        value: o.opt_scalar("value")?.unwrap_or_default(),
        disabled: o.flag("disabled", false)?,
        description: o.description("description")?,
    })
}

fn event(r: &mut Reader, v: &Value, path: &str) -> Res<Event> {
    let o = r.object(v, path, EVENT_KEYS)?;
    let script = match o.get("script") {
        None => Script::default(),
        Some(s) => {
            let so = r.object(s, &o.path("script"), SCRIPT_KEYS)?;
            Script {
                kind: so.opt_str("type")?,
                exec: so.str_list("exec")?,
            }
        }
    };
    Ok(Event {
        listen: o.str_or_empty("listen")?,
        script,
        disabled: o.flag("disabled", false)?,
    })
}

fn variable(r: &mut Reader, v: &Value, path: &str) -> Res<Variable> {
    let o = r.object(v, path, VARIABLE_KEYS)?;
    let key = match o.opt_str("key")? {
        Some(k) => k,
        None => o.str_or_empty("id")?,
    };
    Ok(Variable {
        key,
        value: any_text(o.get("value")),
        kind: o.opt_str("type")?,
        disabled: o.flag("disabled", false)?,
        description: o.description("description")?,
    })
}

fn env_value(r: &mut Reader, v: &Value, path: &str) -> Res<EnvValue> {
    let o = r.object(v, path, ENV_VALUE_KEYS)?;
    let Some(key) = o.opt_str("key")? else {
        return fail(&o.path("key"), "a string");
    };
    Ok(EnvValue {
        key,
        value: any_text(o.get("value")),
        kind: o.opt_str("type")?,
        enabled: o.flag("enabled", true)?,
    })
}

/// Variable values may be any JSON in Postman: scalars as text, objects/arrays as compact JSON.
fn any_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}
