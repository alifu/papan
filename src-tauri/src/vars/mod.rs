//! `{{var}}` resolution, environment layering and secrets (plan 04).
//!
//! `template`, `scopes`, `dynamic` and `resolve` are plain Rust with no Tauri or HTTP
//! types, so every engine can reuse them.

pub mod commands;
pub mod dynamic;
pub mod local;
pub mod resolve;
pub mod scopes;
pub mod service;
pub mod template;
pub mod types;
pub mod vault;
mod write;

pub use service::VarsService;
