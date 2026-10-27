//! `qu kernel` with `"stream": true` sends a run's output as it is produced
//! (`{"op":"out","text":...}` lines) and ends with the normal response, whose
//! `output` is then empty. Qu Studio relies on it: a script that runs for a
//! long time -- or forever -- otherwise shows nothing at all.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn start() -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_qu"))
        .arg("kernel")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn qu kernel")
}

fn send(child: &mut std::process::Child, req: serde_json::Value) {
    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "{req}").unwrap();
    stdin.flush().unwrap();
}

#[test]
fn a_streamed_run_sends_output_before_the_run_ends_and_an_empty_final_output() {
    let mut child = start();
    let mut rd = BufReader::new(child.stdout.take().unwrap());
    // Earlier output must not be streamed again: run something first, unstreamed.
    send(&mut child, serde_json::json!({"op": "run", "code": "print(\"before\")"}));
    let mut line = String::new();
    rd.read_line(&mut line).unwrap();
    let first: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert!(first["output"].as_str().unwrap().contains("before"), "{first}");

    send(
        &mut child,
        serde_json::json!({"op": "run", "stream": true, "code": "print(\"one\")\nprint(\"two\")\nsleep(400)\nprint(\"three\")"}),
    );
    let mut streamed = String::new();
    let mut first_chunk_at = None;
    let start = Instant::now();
    let fin = loop {
        line.clear();
        assert!(rd.read_line(&mut line).unwrap() > 0, "kernel closed early; got {streamed:?}");
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["op"] == "out" {
            first_chunk_at.get_or_insert(start.elapsed());
            streamed.push_str(v["text"].as_str().unwrap());
        } else {
            break v;
        }
    };
    assert!(streamed.contains("one") && streamed.contains("two") && streamed.contains("three"), "{streamed:?}");
    assert!(!streamed.contains("before"), "an earlier run's output was streamed again: {streamed:?}");
    assert_eq!(fin["output"].as_str().unwrap(), "", "{fin}");
    assert_eq!(fin["success"], true, "{fin}");
    // The point of streaming: the first chunk arrives well before the 400 ms sleep ends.
    assert!(
        first_chunk_at.unwrap() < Duration::from_millis(300),
        "first output only after {:?} -- it was held until the run ended",
        first_chunk_at
    );
    let _ = child.kill();
}

#[test]
fn an_unstreamed_run_is_unchanged() {
    let mut child = start();
    let mut rd = BufReader::new(child.stdout.take().unwrap());
    send(&mut child, serde_json::json!({"op": "run", "code": "print(\"hello\")"}));
    let mut line = String::new();
    rd.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["op"], "run", "{v}");
    assert!(v["output"].as_str().unwrap().contains("hello"), "{v}");
    let _ = child.kill();
}

#[test]
fn a_streamed_error_still_reports_the_error_and_the_output_before_it() {
    let mut child = start();
    let mut rd = BufReader::new(child.stdout.take().unwrap());
    send(&mut child, serde_json::json!({"op": "run", "stream": true, "code": "print(\"ok so far\")\nx = undefined_thing + 1"}));
    let mut line = String::new();
    let mut streamed = String::new();
    let fin = loop {
        line.clear();
        assert!(rd.read_line(&mut line).unwrap() > 0);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["op"] == "out" {
            streamed.push_str(v["text"].as_str().unwrap());
        } else {
            break v;
        }
    };
    assert!(streamed.contains("ok so far"), "{streamed:?}");
    assert_eq!(fin["success"], false, "{fin}");
    assert!(fin["error"].as_str().unwrap().contains("undefined_thing"), "{fin}");
    let _ = child.kill();
}

#[test]
fn a_run_with_cwd_resolves_relative_paths_next_to_that_folder() {
    let dir = std::env::temp_dir().join(format!("qu_kernel_cwd_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = start();
    let mut rd = BufReader::new(child.stdout.take().unwrap());
    send(
        &mut child,
        serde_json::json!({"op": "run", "cwd": dir.to_string_lossy(), "code": "write_text(\"marker.txt\", \"here\")"}),
    );
    let mut line = String::new();
    rd.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["success"], true, "{v}");
    assert!(dir.join("marker.txt").exists(), "relative write did not land in the cwd folder");
    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
}
