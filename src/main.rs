//! rolesmith: roles for any coding agent. The contract is SPEC.md.

fn main() -> std::process::ExitCode {
    rolesmith::cli::main(std::env::args_os())
}
