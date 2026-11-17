//! Body and auth readers (split from `parse.rs` for size).

use serde_json::{Map, Value};

use super::parse::{list, opt};
use super::read::{fail, index, Obj, Reader, Res};
use super::{Auth, AuthParam, Body, BodyOptions, FileBody, FormParam, GraphqlBody, RawOptions};

const BODY_KEYS: &[&str] = &[
    "mode",
    "raw",
    "urlencoded",
    "formdata",
    "file",
    "graphql",
    "options",
    "disabled",
];
const FORM_KEYS: &[&str] = &[
    "key",
    "value",
    "src",
    "type",
    "contentType",
    "disabled",
    "description",
];

pub(super) fn body(r: &mut Reader, v: &Value, path: &str) -> Res<Body> {
    let o = r.object(v, path, BODY_KEYS)?;
    let file = match o.opt_object("file")? {
        Some(m) => Some(FileBody {
            src: Obj {
                map: m,
                path: o.path("file"),
            }
            .opt_str("src")?,
        }),
        None => None,
    };
    Ok(Body {
        mode: o.opt_str("mode")?,
        raw: o.opt_str("raw")?,
        urlencoded: list(r, &o, "urlencoded", form_param)?,
        formdata: list(r, &o, "formdata", form_param)?,
        file,
        graphql: opt(r, &o, "graphql", graphql)?,
        options: raw_options(&o)?,
        disabled: o.flag("disabled", false)?,
    })
}

fn raw_options(o: &Obj<'_>) -> Res<Option<BodyOptions>> {
    let Some(options) = o.opt_object("options")? else {
        return Ok(None);
    };
    let options = Obj {
        map: options,
        path: o.path("options"),
    };
    let Some(raw) = options.opt_object("raw")? else {
        return Ok(None);
    };
    let raw = Obj {
        map: raw,
        path: options.path("raw"),
    };
    Ok(raw.opt_str("language")?.map(|language| BodyOptions {
        raw: RawOptions { language },
    }))
}

fn form_param(r: &mut Reader, v: &Value, path: &str) -> Res<FormParam> {
    let o = r.object(v, path, FORM_KEYS)?;
    Ok(FormParam {
        key: o.str_or_empty("key")?,
        value: o.opt_scalar("value")?,
        src: o.str_list("src")?,
        kind: o.opt_str("type")?,
        content_type: o.opt_str("contentType")?,
        disabled: o.flag("disabled", false)?,
        description: o.description("description")?,
    })
}

fn graphql(r: &mut Reader, v: &Value, path: &str) -> Res<GraphqlBody> {
    let o = r.object(v, path, &["query", "variables"])?;
    let variables = match o.get("variables") {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other @ Value::Object(_)) => other.to_string(),
        Some(_) => return fail(&o.path("variables"), "a string or an object"),
    };
    Ok(GraphqlBody {
        query: o.str_or_empty("query")?,
        variables,
    })
}

pub(super) fn auth(r: &mut Reader, v: &Value, path: &str) -> Res<Auth> {
    let Value::Object(map) = v else {
        return fail(path, "an object");
    };
    // Every other key is an (inactive) auth type's parameter block: known, not counted.
    let o = Obj {
        map,
        path: path.to_string(),
    };
    let Some(kind) = o.opt_str("type")? else {
        return fail(&o.path("type"), "a string");
    };
    let params = match o.get(&kind) {
        None => Vec::new(),
        Some(Value::Array(items)) => auth_list(r, items, &o.path(&kind))?,
        Some(Value::Object(m)) => auth_object(m),
        Some(_) => return fail(&o.path(&kind), "an array or an object"),
    };
    Ok(Auth { kind, params })
}

/// v2.1: `[{key, value, type}]`.
fn auth_list(r: &mut Reader, items: &[Value], path: &str) -> Res<Vec<AuthParam>> {
    items
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let o = r.object(v, &index(path, i), &["key", "value", "type"])?;
            let Some(key) = o.opt_str("key")? else {
                return fail(&o.path("key"), "a string");
            };
            let value = o.map.get("value").cloned().unwrap_or(Value::Null);
            Ok(AuthParam {
                key,
                value,
                kind: o.opt_str("type")?,
            })
        })
        .collect()
}

/// v2.0: `{username: "..", password: ".."}`.
fn auth_object(m: &Map<String, Value>) -> Vec<AuthParam> {
    m.iter()
        .map(|(k, v)| AuthParam {
            key: k.clone(),
            value: v.clone(),
            kind: None,
        })
        .collect()
}
