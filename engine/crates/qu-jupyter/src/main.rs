//! `qu-jupyter` — a Jupyter kernel for Qu.
//!
//! Usage:
//!   qu-jupyter install [--prefix <dir>]   register the "Qu" kernelspec so
//!                                         Jupyter and VS Code's Jupyter
//!                                         extension can find and launch it
//!   qu-jupyter -f <connection_file.json>  run as a kernel (this is how
//!                                         Jupyter itself launches it, per
//!                                         the argv recorded by `install`;
//!                                         not meant to be typed by hand)
//!
//! See `kernel.rs` for the actual protocol/session implementation and
//! `README.md` for the end-to-end setup story.

mod connection;
mod install;
mod kernel;
mod protocol;

use std::path::PathBuf;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("install") {
        let mut prefix = None;
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--prefix" => {
                    i += 1;
                    let path = args.get(i).ok_or("--prefix needs a directory argument")?;
                    prefix = Some(PathBuf::from(path));
                }
                other => return Err(format!("qu-jupyter install: unrecognized argument `{other}`")),
            }
            i += 1;
        }
        return install::run(install::InstallOptions { prefix });
    }

    let connection_file = parse_connection_file_arg(&args)?;
    let conn = connection::ConnectionInfo::load(&connection_file)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("starting async runtime: {e}"))?;
    runtime.block_on(kernel::run(conn))
}

/// Accepts both `-f <path>` / `--connection-file <path>` (the form
/// `jupyter_client` actually writes into a kernelspec's `argv`) and a bare
/// positional path, so `qu-jupyter conn.json` also works for manual testing.
fn parse_connection_file_arg(args: &[String]) -> Result<PathBuf, String> {
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--connection-file" => {
                let path = args.get(i + 1).ok_or("-f needs a connection-file path argument")?;
                return Ok(PathBuf::from(path));
            }
            other if !other.starts_with('-') => return Ok(PathBuf::from(other)),
            _ => {}
        }
        i += 1;
    }
    Err(
        "usage: qu-jupyter -f <connection_file.json>  (or: qu-jupyter install [--prefix <dir>])"
            .to_string(),
    )
}
