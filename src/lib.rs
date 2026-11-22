//! rolesmith: roles for any coding agent. The contract is SPEC.md.
//!
//! `cli` is the frame every command runs in (SPEC.md §8); `commands` is the one place commands are
//! registered; `library` reads and checks a library through the types every command uses.

pub mod cli;
pub mod commands;
pub mod library;
pub mod problem;
pub mod secrets;
