//! Protocol engines. All network traffic runs here, never in the webview.

pub mod graphql;
pub mod grpc;
pub mod http;
pub mod soap;
pub mod ws;
