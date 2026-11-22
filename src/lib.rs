//! rolesmith: roles for any coding agent. The contract is SPEC.md.
//!
//! `cli` is the frame every command runs in (SPEC.md §8); `commands` is the one place commands are
//! registered; `version` is what `--version` says, stamped by build.rs.

pub mod cli;
pub mod commands;
pub mod version;
