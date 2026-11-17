//! Wrapper around the system `git` CLI (plan 10).

pub mod commands;
mod parse;
pub mod runner;
pub mod service;
#[cfg(test)]
mod service_tests;
pub mod types;
