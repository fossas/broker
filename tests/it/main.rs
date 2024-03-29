//! Tests for Broker.
//!
//! Some of these tests test specific error message output.
//! It's okay to update the test if the change to the error message output is _desired_,
//! but these tests exist to make sure that any change to previously shipped error message output is _intentional_.
//!
//! The goal with our error messages is that they are useful and actionable to users.
//! If error text changes too often, or changes in a way that makes the error less understandable,
//! we need to make very sure we want to actually make the change.

#![deny(clippy::invalid_regex)]

mod helper;

mod api_code;
mod args;
mod binary;
mod config;
mod db;
mod debug;
mod fix;
mod fossa_cli;
mod init;
mod queue;
mod remote_git;
