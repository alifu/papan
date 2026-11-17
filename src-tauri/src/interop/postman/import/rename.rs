//! Renames lifted credential variables that clash with variables of the collection an
//! import goes into, so an imported `{{name}}` never points at an unrelated variable.

use std::collections::BTreeMap;

use super::tree::{PlannedCollection, PlannedFolder, PlannedNode};
use crate::error::AppError;
use crate::store::env_model::VarTable;
use crate::store::types::{Auth, Body, KeyValue, RequestContent};

/// Old name → new name.
pub type Renames = BTreeMap<String, String>;

/// `collection` with each lifted variable already in `existing` renamed (`name_2`, `name_3`…).
pub fn rename_clashing(
    collection: &PlannedCollection,
    existing: &VarTable,
) -> Result<(PlannedCollection, Renames), AppError> {
    let mut renames = Renames::new();
    for name in &collection.lifted {
        if !existing.contains_key(name) {
            continue;
        }
        let new = (2..)
            .map(|n| format!("{name}_{n}"))
            .find(|candidate| {
                !existing.contains_key(candidate) && !collection.vars.table.contains_key(candidate)
            })
            .unwrap_or_else(|| name.clone());
        renames.insert(name.clone(), new);
    }
    if renames.is_empty() {
        return Ok((collection.clone(), renames));
    }
    let mut next = collection.clone();
    for (old, new) in &renames {
        if let Some(entry) = next.vars.table.remove(old) {
            next.vars.table.insert(new.clone(), entry);
        }
        if let Some(value) = next.vars.secrets.remove(old) {
            next.vars.secrets.insert(new, &value);
        }
    }
    next.lifted = next
        .lifted
        .iter()
        .map(|name| renames.get(name).unwrap_or(name).clone())
        .collect();
    next.root = rename_folder(&collection.root, &renames)?;
    Ok((next, renames))
}

fn rename_folder(folder: &PlannedFolder, renames: &Renames) -> Result<PlannedFolder, AppError> {
    let children = folder
        .children
        .iter()
        .map(|child| match child {
            PlannedNode::Folder(inner) => Ok(PlannedNode::Folder(rename_folder(inner, renames)?)),
            PlannedNode::Request { entry, file } => {
                let content = rename_content(&file.content()?, renames);
                Ok(PlannedNode::Request {
                    entry: entry.clone(),
                    file: file.with_content(&content)?,
                })
            }
        })
        .collect::<Result<_, AppError>>()?;
    Ok(PlannedFolder {
        children,
        ..folder.clone()
    })
}

fn rename_content(content: &RequestContent, renames: &Renames) -> RequestContent {
    let fix = |value: &str| -> String {
        renames
            .iter()
            .find(|(old, _)| value == format!("{{{{{old}}}}}"))
            .map_or_else(|| value.to_string(), |(_, new)| format!("{{{{{new}}}}}"))
    };
    let auth = match &content.auth {
        Auth::Basic { username, password } => Auth::Basic {
            username: username.clone(),
            password: fix(password),
        },
        Auth::Digest { username, password } => Auth::Digest {
            username: username.clone(),
            password: fix(password),
        },
        Auth::Bearer { token } => Auth::Bearer { token: fix(token) },
        Auth::ApiKey {
            name,
            value,
            placement,
        } => Auth::ApiKey {
            name: name.clone(),
            value: fix(value),
            placement: *placement,
        },
        Auth::Oauth2 {
            grant,
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            client_auth,
            redirect_port,
        } => Auth::Oauth2 {
            grant: *grant,
            auth_url: auth_url.clone(),
            token_url: token_url.clone(),
            client_id: client_id.clone(),
            client_secret: fix(client_secret),
            scope: scope.clone(),
            client_auth: *client_auth,
            redirect_port: *redirect_port,
        },
        other => other.clone(),
    };
    let url = renames.iter().fold(content.url.clone(), |url, (old, new)| {
        url.replace(&format!("{{{{{old}}}}}"), &format!("{{{{{new}}}}}"))
    });
    let body = match &content.body {
        Body::Form { fields } => Body::Form {
            fields: fields
                .iter()
                .map(|field| KeyValue {
                    value: fix(&field.value),
                    ..field.clone()
                })
                .collect(),
        },
        other => other.clone(),
    };
    RequestContent {
        url,
        body,
        headers: content
            .headers
            .iter()
            .map(|header| KeyValue {
                value: fix(&header.value),
                ..header.clone()
            })
            .collect(),
        auth,
        ..content.clone()
    }
}
