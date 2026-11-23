//! rolesmith: roles for any coding agent. The contract is SPEC.md.
//!
//! `cli` is the frame every command runs in (SPEC.md §8); `commands` is the one place commands are
//! registered; `library` reads and checks a library through the types every command uses; `machine`
//! answers what this machine has of what a role needs; `install` installs a tool by its pinned method;
//! `version` is what `--version` says, stamped by build.rs; `help` is the manual every `--help`
//! answers from.

agent_help::binary!("rolesmith");

pub mod cli;
pub mod commands;
pub mod entry_file;
pub mod github;
pub mod help;
pub mod install;
pub mod library;
pub mod machine;
pub mod problem;
pub mod project;
pub mod secrets;
pub mod sources;
pub mod suggestions;
pub mod upstream;
pub mod version;
