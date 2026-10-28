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

// ---- lazy variable payload: a large numeric variable is a preview, fetched whole on request ----

fn reply(rd: &mut BufReader<std::process::ChildStdout>) -> (serde_json::Value, usize) {
    let mut line = String::new();
    assert!(rd.read_line(&mut line).unwrap() > 0, "kernel closed early");
    (serde_json::from_str(&line).unwrap(), line.len())
}

fn entry<'a>(run: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    run["data"].as_array().unwrap().iter().find(|e| e["name"] == name).unwrap_or_else(|| panic!("no data entry for {name}"))
}

#[test]
fn a_large_variable_is_sent_as_a_preview_with_exact_whole_array_stats() {
    let mut child = start();
    let mut rd = BufReader::new(child.stdout.take().unwrap());
    send(&mut child, serde_json::json!({"op": "run", "code": "v = 0:199999\ns = [1, 2, 3]"}));
    let (run, bytes) = reply(&mut rd);
    // 200,000 values used to be ~1.4 MB of JSON in every reply; now a few tens of KB.
    assert!(bytes < 100_000, "reply is {bytes} bytes");
    let v = entry(&run, "v");
    assert_eq!(v["truncated"], true, "{v}");
    assert_eq!(v["length"], 200_000);
    assert_eq!(v["shape"], serde_json::json!([200_000, 1]));
    let head = v["data"].as_array().unwrap();
    assert_eq!(head.len(), 2000);
    assert_eq!(head[0], 0.0);
    assert_eq!(head[1999], 1999.0);
    // stats cover the WHOLE array, not the preview
    assert_eq!(v["stats"]["min"], 0.0);
    assert_eq!(v["stats"]["max"], 199_999.0);
    assert!((v["stats"]["mean"].as_f64().unwrap() - 99_999.5).abs() < 1e-6, "{}", v["stats"]);
    assert_eq!(v["stats"]["count"], 200_000);
    // the profile spans the whole array (1-based indices) and keeps the real extremes
    let sample = v["sample"].as_array().unwrap();
    assert!(sample.len() >= 100 && sample.len() <= 600, "{}", sample.len());
    assert_eq!(sample[0][0], 1);
    assert_eq!(sample[sample.len() - 1][0], 200_000);
    assert!(sample.iter().any(|p| p[1] == 199_999.0));
    // a small variable is exactly what it always was: whole, no preview fields
    let s = entry(&run, "s");
    assert_eq!(s["data"], serde_json::json!([1.0, 2.0, 3.0]));
    assert!(s.get("truncated").is_none() && s.get("stats").is_none(), "{s}");
    let _ = child.kill();
}

#[test]
fn the_var_request_returns_any_slice_of_a_previewed_variable() {
    let mut child = start();
    let mut rd = BufReader::new(child.stdout.take().unwrap());
    send(&mut child, serde_json::json!({"op": "run", "code": "v = 0:199999\nr = {a = linspace(0, 19999, 20000), b = 1}"}));
    let (setup, _) = reply(&mut rd);
    assert_eq!(setup["success"], true, "the setup script failed: {setup}");

    send(&mut child, serde_json::json!({"op": "var", "name": "v", "start": 10, "count": 5}));
    let (r, _) = reply(&mut rd);
    assert_eq!(r["op"], "var");
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["data"], serde_json::json!([10.0, 11.0, 12.0, 13.0, 14.0]));
    assert_eq!(r["length"], 200_000);
    assert_eq!(r["start"], 10);

    // no count: everything (here under the per-request cap)
    send(&mut child, serde_json::json!({"op": "var", "name": "v"}));
    let (r, _) = reply(&mut rd);
    assert_eq!(r["data"].as_array().unwrap().len(), 200_000);

    // a slice that runs past the end is cut, one that starts past it is empty
    send(&mut child, serde_json::json!({"op": "var", "name": "v", "start": 199_998, "count": 100}));
    let (r, _) = reply(&mut rd);
    assert_eq!(r["data"], serde_json::json!([199_998.0, 199_999.0]));
    send(&mut child, serde_json::json!({"op": "var", "name": "v", "start": 999_999}));
    let (r, _) = reply(&mut rd);
    assert_eq!(r["data"], serde_json::json!([]));

    // a record's numeric field is addressed as name.field, like the data entries
    send(&mut child, serde_json::json!({"op": "var", "name": "r.a", "start": 5, "count": 2}));
    let (r, _) = reply(&mut rd);
    assert_eq!(r["data"], serde_json::json!([5.0, 6.0]), "{r}");

    // an unknown name is a clear failure, not a hang or a crash
    send(&mut child, serde_json::json!({"op": "var", "name": "nope"}));
    let (r, _) = reply(&mut rd);
    assert_eq!(r["success"], false);
    assert!(r["error"].as_str().unwrap().contains("nope"), "{r}");
    // and the kernel still works afterwards
    send(&mut child, serde_json::json!({"op": "run", "code": "print(1)"}));
    let (r, _) = reply(&mut rd);
    assert_eq!(r["success"], true);
    let _ = child.kill();
}

#[test]
fn a_big_matrix_and_non_finite_values_survive_the_preview() {
    let mut child = start();
    let mut rd = BufReader::new(child.stdout.take().unwrap());
    send(&mut child, serde_json::json!({"op": "run", "code": "M = zeros(300, 100)\nM[0, 0] = 7\nw = 0:19999\nw[3] = nan"}));
    let (run, _) = reply(&mut rd);
    assert_eq!(run["success"], true, "the setup script failed: {run}");
    let m = entry(&run, "M");
    assert_eq!(m["truncated"], true);
    assert_eq!(m["shape"], serde_json::json!([300, 100]), "the real shape, not the preview's");
    assert_eq!(m["length"], 30_000);
    assert_eq!(m["data"][0], 7.0);
    assert_eq!(m["stats"]["max"], 7.0);
    // a NaN in the full array is counted as missing and never breaks the JSON
    let w = entry(&run, "w");
    assert_eq!(w["stats"]["missing"], 1);
    assert_eq!(w["stats"]["count"], 19_999);
    assert!(w["data"][3].is_null(), "{}", w["data"][3]);
    let _ = child.kill();
}
