//! Integration tests for `qu run`'s `--max-time`/`--max-memory`/
//! `--sandbox`/`--profile` flags (see `qu-cli/src/resource.rs`'s module
//! doc comment for the watchdog-thread design, and
//! `qu_interp::Interp::check_sandbox`/`call_tracked` for the sandbox/
//! profiling implementations). These spawn the actual compiled `qu`
//! binary as a subprocess against a real temp script — the flag parsing,
//! process-kill behavior, and stderr/stdout wiring these exercise can only
//! be observed end-to-end through `qu-cli`'s own `main`, not through
//! `qu-interp`'s unit tests (which never go through `cmd_run` at all).

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn qu_bin() -> &'static str {
    env!("CARGO_BIN_EXE_qu")
}

fn write_script(name: &str, src: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("qu_cli_resource_flag_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(src.as_bytes()).unwrap();
    path
}

// ---- Task 1: --max-time ----

#[test]
fn max_time_kills_a_real_infinite_loop_with_a_clear_message() {
    let path = write_script("infinite_loop.qu", "x = 0\nwhile true\n  x = x + 1\nend\n");
    let start = Instant::now();
    let out = Command::new(qu_bin())
        .args(["run", path.to_str().unwrap(), "--max-time", "1"])
        .output()
        .expect("failed to run qu");
    let elapsed = start.elapsed();
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(!out.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("killed: exceeded --max-time 1s"),
        "expected a clear kill message naming the limit, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("ran "),
        "expected the actual measured elapsed time in the message, got: {stderr}"
    );
    // Real proof the watchdog actually killed it, rather than the loop
    // somehow finishing on its own: comfortably bounded, not "however
    // long an un-killed infinite loop would otherwise run."
    assert!(elapsed < Duration::from_secs(15), "took {elapsed:?} — watchdog did not fire promptly");
}

// ---- Task 1: --max-memory ----

#[test]
fn max_memory_kills_a_real_memory_hungry_script() {
    // Grows a real, live matrix by a fixed ~1.6MB chunk (100,000 f64
    // elements) each iteration via `vstack` (old data is kept, not
    // replaced), so RSS climbs gradually and smoothly past a small cap --
    // deliberately NOT exponential growth, which would jump straight from
    // "comfortably under the cap" to "single multi-gigabyte allocation
    // request" between two 25ms polls and abort the process with a plain
    // allocator-failure panic instead of ever giving the watchdog a
    // chance to observe an intermediate RSS reading and kill it cleanly.
    // `--max-time` is a generous backstop only, so a genuine failure to
    // enforce `--max-memory` shows up as a `--max-time` message instead of
    // a hang, rather than the test itself hanging forever.
    let path = write_script(
        "memory_hog.qu",
        "v = zeros(1, 100000)\nwhile true\n  v = vstack(v, zeros(1, 100000))\nend\n",
    );
    let start = Instant::now();
    let out = Command::new(qu_bin())
        .args(["run", path.to_str().unwrap(), "--max-memory", "64", "--max-time", "20"])
        .output()
        .expect("failed to run qu");
    let elapsed = start.elapsed();
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(!out.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("killed: exceeded --max-memory 64MB"),
        "expected the memory cap to trip before the 20s time backstop, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("used ~"),
        "expected the actual measured RSS in the message, got: {stderr}"
    );
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");
}

// ---- Task 2: --profile ----

#[test]
fn profile_attributes_more_time_to_a_hot_function_than_a_cold_one() {
    let script = "\
function hot(n)
    total = 0
    for i = 1 to n
        total = total + sqrt(i)
    end
    return total
end function

function cold(n)
    total = 0
    for i = 1 to n
        total = total + i
    end
    return total
end function

x = hot(500000)
y = cold(2)
print(\"{x}, {y}\")
";
    let path = write_script("profile_hot_cold.qu", script);
    let out = Command::new(qu_bin())
        .args(["run", path.to_str().unwrap(), "--profile"])
        .output()
        .expect("failed to run qu");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");

    assert!(stderr.contains("qu profile"), "stderr: {stderr}");
    assert!(stderr.contains("peak RSS"), "stderr: {stderr}");
    assert!(stderr.contains("hot"), "stderr: {stderr}");
    assert!(stderr.contains("cold"), "stderr: {stderr}");

    let hot_line = stderr
        .lines()
        .find(|l| l.trim_start().starts_with("hot "))
        .unwrap_or_else(|| panic!("no `hot` row in profile output:\n{stderr}"));
    let cold_line = stderr
        .lines()
        .find(|l| l.trim_start().starts_with("cold "))
        .unwrap_or_else(|| panic!("no `cold` row in profile output:\n{stderr}"));

    let total_time_field = |line: &str| -> f64 {
        // columns: name, calls, "<secs>s", "<pct>%"
        let field = line.split_whitespace().nth(2).unwrap();
        field.trim_end_matches('s').parse().unwrap()
    };
    let hot_secs = total_time_field(hot_line);
    let cold_secs = total_time_field(cold_line);
    assert!(
        hot_secs > cold_secs,
        "expected hot() (2,000,000 iterations) to show more total time than cold() (2 iterations); \
         hot={hot_secs}s cold={cold_secs}s\nfull report:\n{stderr}"
    );
}

#[test]
fn profile_output_writes_the_report_to_a_file_instead_of_stderr() {
    let path = write_script("profile_to_file.qu", "function f(n)\n  return n * 2\nend function\nprint(f(21))\n");
    let out_dir = std::env::temp_dir().join("qu_cli_resource_flag_tests");
    std::fs::create_dir_all(&out_dir).unwrap();
    let report_path = out_dir.join("profile_report.txt");
    let _ = std::fs::remove_file(&report_path);

    let out = Command::new(qu_bin())
        .args([
            "run",
            path.to_str().unwrap(),
            "--profile",
            "--profile-output",
            report_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run qu");
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("qu profile"), "report should go to the file, not stderr: {stderr}");

    let report = std::fs::read_to_string(&report_path).expect("--profile-output file was not written");
    assert!(report.contains("qu profile"));
    assert!(report.contains('f'));
}

// ---- Task 3: --sandbox ----

#[test]
fn sandbox_blocks_http_get_but_leaves_ordinary_code_running() {
    let path = write_script(
        "sandbox_mixed.qu",
        "x = 1 + 2\nprint(\"ok: {x}\")\nhttp_get(\"http://example.invalid/\")\n",
    );
    let out = Command::new(qu_bin())
        .args(["run", path.to_str().unwrap(), "--sandbox"])
        .output()
        .expect("failed to run qu");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(stdout.contains("ok: 3"), "the allowed statements before the denied call must still run: {stdout}");
    assert!(
        stderr.contains("sandbox: 'http_get' is disabled in sandboxed execution"),
        "stderr: {stderr}"
    );
    assert!(!out.status.success());
}

#[test]
fn sandbox_blocks_every_deny_listed_network_and_process_builtin() {
    for name in ["tcp_listen", "tcp_connect", "listen_pool", "python_exec"] {
        // Each is called with placeholder/garbage arguments -- the sandbox
        // check runs before argument validation (it's the very first thing
        // `call_builtin` does), so it must reject these before ever
        // getting far enough to complain about bad arguments instead.
        let script = format!("{name}(0)\n");
        let path = write_script(&format!("sandbox_deny_{name}.qu"), &script);
        let out = Command::new(qu_bin())
            .args(["run", path.to_str().unwrap(), "--sandbox"])
            .output()
            .expect("failed to run qu");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("sandbox: '{name}' is disabled in sandboxed execution")),
            "builtin `{name}` was not rejected by --sandbox; stderr: {stderr}"
        );
        assert!(!out.status.success());
    }
}

#[test]
fn sandbox_blocks_write_csv_touch_and_write_mode_fopen() {
    let dir = std::env::temp_dir().join("qu_cli_resource_flag_tests");
    std::fs::create_dir_all(&dir).unwrap();

    let touch_target = dir.join("should_not_be_touched.txt");
    let _ = std::fs::remove_file(&touch_target);
    let touch_script = format!("touch(\"{}\")\n", escape_qu_string_path(&touch_target));
    let path = write_script("sandbox_touch.qu", &touch_script);
    let out = Command::new(qu_bin())
        .args(["run", path.to_str().unwrap(), "--sandbox"])
        .output()
        .expect("failed to run qu");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("sandbox: 'touch' is disabled in sandboxed execution"), "stderr: {stderr}");
    assert!(!touch_target.exists(), "sandboxed touch() must not create the file");

    let fopen_target = dir.join("should_not_be_written.csv");
    let _ = std::fs::remove_file(&fopen_target);
    let fopen_script = format!("f = fopen(\"{}\", \"w\")\n", escape_qu_string_path(&fopen_target));
    let path = write_script("sandbox_fopen_write.qu", &fopen_script);
    let out = Command::new(qu_bin())
        .args(["run", path.to_str().unwrap(), "--sandbox"])
        .output()
        .expect("failed to run qu");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("'fopen' in write mode"), "stderr: {stderr}");
    assert!(!fopen_target.exists(), "sandboxed fopen(\"w\") must not create the file");
}

#[test]
fn sandbox_allows_file_reads_like_read_csv() {
    let dir = std::env::temp_dir().join("qu_cli_resource_flag_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let csv_path = dir.join("sandbox_readable.csv");
    std::fs::write(&csv_path, "a,b\n1,2\n3,4\n").unwrap();

    let script = format!(
        "df = read_csv(\"{}\")\nprint(\"rows: {{nrow(df)}}\")\n",
        escape_qu_string_path(&csv_path)
    );
    let path = write_script("sandbox_read_csv.qu", &script);
    let out = Command::new(qu_bin())
        .args(["run", path.to_str().unwrap(), "--sandbox"])
        .output()
        .expect("failed to run qu");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(out.status.success(), "sandboxed file READ must still work; stderr: {stderr}");
    assert!(stdout.contains("rows: 2"), "stdout: {stdout}");
}

#[test]
fn without_sandbox_the_same_denied_builtin_call_is_a_plain_argument_error_not_a_sandbox_error() {
    // Confirms --sandbox is opt-in: the same call, run WITHOUT --sandbox,
    // must never mention "disabled in sandboxed execution" (it's free to
    // fail for its own ordinary reasons, e.g. an unreachable host/bad URL
    // — this only checks the sandbox wording is absent).
    let path = write_script("no_sandbox_http_get.qu", "http_get(\"http://example.invalid/\")\n");
    let out = Command::new(qu_bin())
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("failed to run qu");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("disabled in sandboxed execution"), "stderr: {stderr}");
}

fn escape_qu_string_path(p: &std::path::Path) -> String {
    p.to_str().unwrap().replace('\\', "\\\\")
}
