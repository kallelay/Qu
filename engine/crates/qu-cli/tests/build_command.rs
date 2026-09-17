//! Integration tests for `qu build <file.qu> [-o <output>]` — "hard
//! compile" (see BACKLOG.md's soft/hard compile distinction), which
//! produces a standalone executable by appending the script's bytes plus a
//! magic+length footer to a copy of the currently-running `qu` binary (see
//! `qu-cli/src/main.rs`'s module doc comment and `cmd_build`/
//! `read_embedded_script`/`run_embedded_script`). These spawn the actual
//! compiled `qu` binary as a subprocess — building, then running the
//! *resulting* binary as its own subprocess — since the whole point is
//! behavior only observable by actually executing a built artifact, not
//! anything `qu-interp`'s unit tests could exercise.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn qu_bin() -> &'static str {
    env!("CARGO_BIN_EXE_qu")
}

fn write_script(name: &str, src: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("qu_cli_build_command_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(src.as_bytes()).unwrap();
    path
}

fn out_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("qu_cli_build_command_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn built_binary_produces_the_same_output_as_qu_run_on_the_same_script() {
    let script = write_script(
        "trivial.qu",
        "x = 1 + 2\nprint(\"hello from built binary: {x}\")\n",
    );
    let exe_out = out_path(if cfg!(windows) { "trivial_built.exe" } else { "trivial_built" });

    let build = Command::new(qu_bin())
        .args(["build", script.to_str().unwrap(), "-o", exe_out.to_str().unwrap()])
        .output()
        .expect("failed to run qu build");
    assert!(
        build.status.success(),
        "qu build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(exe_out.exists(), "qu build did not write {}", exe_out.display());

    let run_ref = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap()])
        .output()
        .expect("failed to run qu run");
    assert!(run_ref.status.success());

    let built_run = Command::new(&exe_out).output().expect("failed to run the built binary");
    assert!(
        built_run.status.success(),
        "built binary failed: {}",
        String::from_utf8_lossy(&built_run.stderr)
    );

    assert_eq!(
        String::from_utf8_lossy(&built_run.stdout),
        String::from_utf8_lossy(&run_ref.stdout)
    );
}

#[test]
fn built_binary_forwards_its_own_argv_to_the_script() {
    let script = write_script(
        "argv_echo.qu",
        "a = argv()\nprint(\"count={len(a)} first={a[0]}\")\n",
    );
    let exe_out = out_path(if cfg!(windows) { "argv_echo_built.exe" } else { "argv_echo_built" });

    let build = Command::new(qu_bin())
        .args(["build", script.to_str().unwrap(), "-o", exe_out.to_str().unwrap()])
        .output()
        .expect("failed to run qu build");
    assert!(build.status.success(), "stderr: {}", String::from_utf8_lossy(&build.stderr));

    let out = Command::new(&exe_out)
        .args(["hello", "world"])
        .output()
        .expect("failed to run the built binary");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("count=2"), "stdout: {stdout}");
    assert!(stdout.contains("first=hello"), "stdout: {stdout}");
}

#[test]
fn qu_build_without_dash_o_derives_the_output_path_from_the_script_name() {
    let script = write_script("derived_name.qu", "print(\"ok\")\n");
    let expected = script.with_extension(if cfg!(windows) { "exe" } else { "" });
    let expected = if cfg!(windows) {
        expected
    } else {
        script.parent().unwrap().join("derived_name")
    };
    let _ = std::fs::remove_file(&expected);

    let build = Command::new(qu_bin())
        .args(["build", script.to_str().unwrap()])
        .output()
        .expect("failed to run qu build");
    assert!(build.status.success(), "stderr: {}", String::from_utf8_lossy(&build.stderr));
    assert!(expected.exists(), "expected default output at {}", expected.display());
}

#[test]
fn a_plain_qu_binary_with_no_footer_behaves_exactly_as_before() {
    // Guards the other half of the feature: the footer check that makes a
    // built binary self-running must be a true no-op for an ordinary `qu`
    // executable — no footer present, so `run` falls straight through to
    // normal argv dispatch exactly as it always has.
    let script = write_script("no_footer_check.qu", "print(\"plain qu still works\")\n");
    let out = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap()])
        .output()
        .expect("failed to run qu run");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "plain qu still works\n");

    // `qu` with no subcommand at all still prints help, not an attempt to
    // read a nonexistent embedded script.
    let help = Command::new(qu_bin()).output().expect("failed to run qu with no args");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("commands:"));
}

#[test]
fn a_built_binarys_own_argv_is_entirely_the_scripts_not_qu_subcommand_dispatch() {
    // The footer check in `run()` fires before argv dispatch, so a built
    // binary never reaches `cmd_run`/`cmd_build`/etc's own flag parsing —
    // every argv entry, even one that looks like a `qu` subcommand, is
    // just the script's `argv()`. Confirms that by passing `build ...`
    // arguments to a built binary and checking they show up as plain
    // script args instead of triggering a nested build.
    let script = write_script("stub_source.qu", "print(\"argv: {argv()}\")\n");
    let stub_out = out_path(if cfg!(windows) { "stub.exe" } else { "stub" });

    let build = Command::new(qu_bin())
        .args(["build", script.to_str().unwrap(), "-o", stub_out.to_str().unwrap()])
        .output()
        .expect("failed to run qu build");
    assert!(build.status.success());

    let would_be_nested_output = out_path(if cfg!(windows) { "nested.exe" } else { "nested" });
    let run = Command::new(&stub_out)
        .args(["build", "other.qu", "-o", would_be_nested_output.to_str().unwrap()])
        .output()
        .expect("failed to run the built binary");
    assert!(run.status.success(), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("build"), "stdout: {stdout}");
    assert!(!would_be_nested_output.exists(), "argv must not have triggered a nested `qu build`");
}
