use std::io::{self, BufRead, Write};
use std::process::ExitCode;

/// The desktop renders snapshots; one persistent interpreter handles events.
pub fn run(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else { eprintln!("usage: qu gui <file.qu> (JSON host protocol on stdin/stdout)"); return ExitCode::FAILURE; };
    let source = match std::fs::read_to_string(path) { Ok(source) => source, Err(error) => { eprintln!("{error}"); return ExitCode::FAILURE; } };
    let mut interpreter = qu_interp::Interp::new();
    if let Err(error) = interpreter.run(&source) { eprintln!("{error}"); return ExitCode::FAILURE; }
    println!("{}", interpreter.gui_packet());
    if io::stdout().flush().is_err() { return ExitCode::FAILURE; }
    for line in io::stdin().lock().lines() {
        let line = match line { Ok(line) => line, Err(_) => break };
        if line.len() > 1_000_000 { eprintln!("GUI event exceeds 1 MB"); return ExitCode::FAILURE; }
        println!("{}", interpreter.gui_dispatch_json(&line));
        if io::stdout().flush().is_err() { break; }
    }
    ExitCode::SUCCESS
}
