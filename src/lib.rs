//! This library exists to decouple functionality from frontend UX, in an attempt to make testing easier
//! and lead to better overall maintainability.
//!
//! While it is possible to import this library from another Rust program, this library
//! may make major breaking changes on _any_ release, as it is not considered part of the API contract
//! for Broker (which is distributed to end users in binary form only).

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

pub mod api;
pub mod config;
pub mod db;
pub mod debug;
pub mod doc;
pub mod ext;
pub mod queue;
pub mod subcommand;
