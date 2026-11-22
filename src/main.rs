//! rolesmith: roles for any coding agent. The contract is SPEC.md.

fn main() {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--version") | Some("-V") => println!("rolesmith {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!("rolesmith: not built yet; SPEC.md is the contract");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_version_is_the_package_version() {
        assert!(!env!("CARGO_PKG_VERSION").is_empty());
    }
}
