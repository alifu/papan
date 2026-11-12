//! HTTP engine (REST, GraphQL, SOAP) built on `reqwest` (plan 03-http §6).
//!
//! `types` is the IPC contract and `commands` the thin Tauri adapters; `service` is the
//! plain-Rust engine they call.

pub mod build;
pub mod client;
pub mod commands;
pub mod grants;
pub mod inflight;
pub mod redirect;
pub mod responses;
pub mod send;
pub mod service;
pub mod types;

pub use service::HttpEngine;
