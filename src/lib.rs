//! rolesmith: roles for any coding agent. The contract is SPEC.md.
//!
//! `cli` is the frame every command runs in (SPEC.md §8); `commands` is the one place commands are
//! registered; `library` reads and checks a library through the types every command uses; `needs`
//! is what roles need of a machine and `machine` answers what a host has of it, `host` running a
//! shell line here or over ssh, `probe` reading a mapped service's command under a bound; `install` installs a tool by its pinned method;
//! `version` is what `--version` says, stamped by build.rs; `help` is the manual every `--help`
//! answers from.

agent_help::binary!("rolesmith");

pub mod brew;
pub mod cli;
pub mod commands;
pub mod entry_file;
pub mod github;
pub mod help;
pub mod host;
pub mod install;
pub mod library;
pub mod machine;
pub mod needs;
pub mod probe;
pub mod problem;
pub mod project;
pub mod secrets;
pub mod service_map;
pub mod sources;
pub mod suggestions;
pub mod upstream;
pub mod version;
