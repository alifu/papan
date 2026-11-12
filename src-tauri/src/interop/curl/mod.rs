//! cURL import and export (plan 03b §7). Import only parses; export never holds a vault
//! value.

mod build;
pub mod export;
mod export_check;
pub mod import;
pub mod masked;
mod options;
mod quote;
pub mod words;

pub use export::{export, OAUTH_TOKEN_PLACEHOLDER};
pub use import::{import, MAX_INPUT_BYTES};
pub use masked::export_masked;
