//! The user's preview choices applied to a plan: which variables are secret, and whether
//! scripts come along at all (they are always switched off when they do, plan 09 D7).

use super::tree::{PlannedFolder, PlannedNode};
use super::vars::apply_choice;
use super::{ImportPlan, PlannedEnv};
use crate::interop::postman::types::ImportOptions;
use crate::store::request_model::Extra;
use crate::store::script_model::SCRIPTS;

pub fn with_options(plan: &ImportPlan, options: &ImportOptions) -> ImportPlan {
    let names = &options.secret_names;
    let collection = plan.collection.as_ref().map(|collection| {
        let chosen = super::tree::PlannedCollection {
            vars: apply_choice(&collection.vars, names),
            ..collection.clone()
        };
        if options.import_scripts {
            return chosen;
        }
        super::tree::PlannedCollection {
            file: crate::store::model::CollectionFile {
                extra: strip(&chosen.file.extra),
                ..chosen.file.clone()
            },
            root: strip_folder(&chosen.root),
            scripts: 0,
            ..chosen
        }
    });
    ImportPlan {
        collection,
        environments: plan
            .environments
            .iter()
            .map(|env| PlannedEnv {
                vars: apply_choice(&env.vars, names),
                ..env.clone()
            })
            .collect(),
        globals: plan.globals.as_ref().map(|g| apply_choice(g, names)),
        summaries: plan.summaries.clone(),
    }
}

fn strip(extra: &Extra) -> Extra {
    extra
        .iter()
        .filter(|(key, _)| key.as_str() != SCRIPTS)
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn strip_folder(folder: &PlannedFolder) -> PlannedFolder {
    PlannedFolder {
        dir: folder.dir.clone(),
        file: crate::store::model::FolderFile {
            extra: strip(&folder.file.extra),
            ..folder.file.clone()
        },
        children: folder
            .children
            .iter()
            .map(|child| match child {
                PlannedNode::Folder(inner) => PlannedNode::Folder(strip_folder(inner)),
                PlannedNode::Request { entry, file } => PlannedNode::Request {
                    entry: entry.clone(),
                    file: crate::store::model::RequestFile {
                        extra: strip(&file.extra),
                        ..file.clone()
                    },
                },
            })
            .collect(),
    }
}
