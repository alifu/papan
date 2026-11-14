//! HTTP engine (REST, GraphQL, SOAP) built on `reqwest` (plan 03-http §6).
//!
//! `types` is the IPC contract and `commands` the thin Tauri adapters; `service` is the
//! plain-Rust engine they call.

pub mod build;
pub mod client;
pub mod commands;
pub mod cookie_file;
pub mod cookie_jars;
pub mod cookies;
pub mod digest;
mod follow;
pub mod grants;
pub mod identity;
pub mod inflight;
mod mask;
pub mod net_commands;
pub mod network;
pub mod oauth;
pub mod protocol_body;
pub mod proxy;
mod proxy_secret;
pub mod redirect;
pub mod resolve;
pub mod responses;
mod script_bridge;
mod script_flags;
pub mod script_map;
pub mod scripted;
pub mod send;
mod send_guard;
pub mod service;
pub mod timing;
pub mod types;
pub mod variables;

pub use service::HttpEngine;
