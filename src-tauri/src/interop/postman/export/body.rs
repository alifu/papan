//! Our request body → Postman v2.1 `body` (plan 09 §3 request table).

use super::Report;
use crate::interop::postman::schema::{
    Body as PmBody, BodyOptions, FileBody, FormParam, GraphqlBody, RawOptions,
};
use crate::interop::postman::types::ReportLevel;
use crate::store::types::{Body, KeyValue, Part};

pub fn body(body: &Body, path: &str, report: &mut Report) -> Option<PmBody> {
    match body {
        Body::None => None,
        Body::Json { text } => Some(raw(text, "json")),
        Body::Xml { text } => Some(raw(text, "xml")),
        Body::Text { text } => Some(raw(text, "text")),
        Body::Form { fields } => Some(PmBody {
            mode: Some("urlencoded".into()),
            urlencoded: fields.iter().map(form_field).collect(),
            ..PmBody::default()
        }),
        Body::Multipart { parts } => Some(PmBody {
            mode: Some("formdata".into()),
            formdata: parts.iter().map(part).collect(),
            ..PmBody::default()
        }),
        Body::Binary { file } => Some(PmBody {
            mode: Some("file".into()),
            file: Some(FileBody { src: file.clone() }),
            ..PmBody::default()
        }),
        Body::Graphql {
            query,
            variables,
            operation_name,
        } => {
            if !operation_name.is_empty() {
                report.add(
                    ReportLevel::Info,
                    path,
                    "Postman has no GraphQL operation name; it was not exported.",
                );
            }
            Some(PmBody {
                mode: Some("graphql".into()),
                graphql: Some(GraphqlBody {
                    query: query.clone(),
                    variables: variables.clone(),
                }),
                ..PmBody::default()
            })
        }
        Body::Unsupported => {
            report.add(
                ReportLevel::Skipped,
                path,
                "A body made by a newer Papan was not exported.",
            );
            None
        }
    }
}

fn raw(text: &str, language: &str) -> PmBody {
    PmBody {
        mode: Some("raw".into()),
        raw: Some(text.to_string()),
        options: Some(BodyOptions {
            raw: RawOptions {
                language: language.to_string(),
            },
        }),
        ..PmBody::default()
    }
}

fn form_field(field: &KeyValue) -> FormParam {
    FormParam {
        key: field.name.clone(),
        value: Some(field.value.clone()),
        disabled: !field.enabled,
        ..FormParam::default()
    }
}

fn part(part: &Part) -> FormParam {
    match part.file.as_deref().filter(|file| !file.is_empty()) {
        Some(file) => FormParam {
            key: part.name.clone(),
            src: vec![file.to_string()],
            kind: Some("file".into()),
            disabled: !part.enabled,
            ..FormParam::default()
        },
        None => FormParam {
            key: part.name.clone(),
            value: Some(part.value.clone()),
            kind: Some("text".into()),
            disabled: !part.enabled,
            ..FormParam::default()
        },
    }
}
