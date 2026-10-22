//! Library support for Kani's extension-development command-line tool.
//!
//! It exposes declarative code generation, repository signing, build orchestration, and REPL
//! helpers; CLI presentation and argument parsing live in [`commands`].

pub mod codegen;
pub mod commands;
pub mod dsl;
pub mod error;
pub mod repl;
pub mod signing;
pub mod yaml;
