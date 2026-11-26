//! rolesmith: roles for any coding agent. Its manual is `rolesmith help all`; its decisions are
//! in docs/adr/.

fn main() -> std::process::ExitCode {
    rolesmith::cli::main(std::env::args_os())
}
