//! GraphQL helpers that are not sends: schema introspection and the schema cache
//! (plan 06 §6). Sending a GraphQL request goes through the HTTP pipeline.

pub mod cache;
pub mod commands;
pub mod driver;
pub mod gql_ws;
pub mod introspection;
pub mod service;
pub mod subscription;
pub mod subscription_commands;
pub mod subscription_service;
pub mod types;

pub use cache::SchemaCache;

/// Managed by Tauri: the schema cache under the app data directory.
#[derive(Debug)]
pub struct GraphqlState {
    pub cache: SchemaCache,
}
