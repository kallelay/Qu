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

// ---------------------------------------------------------------------
// Bundled imports.
//
// `qu build` used to embed ONLY the entry script. A project with
// `import "lib/x.qu"` therefore built with exit 0, ran correctly *in its
// own source directory* (where `run_embedded_script` points import
// resolution -- beside the binary), and failed the moment the executable
// was copied anywhere else. The build machine said success; the target
// machine said `os error 3`.
//
// Every test below therefore runs the built binary from a directory that
// contains NOTHING but the binary itself. Running it in place would pass
// against the old, broken behaviour too -- a check that never reaches its
// subject.
// ---------------------------------------------------------------------

/// A fresh project directory plus a separate, empty "distribution"
/// directory, both unique to `tag` so concurrent tests cannot collide.
fn project_dirs(tag: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join("qu_cli_bundle_tests").join(tag);
    let _ = std::fs::remove_dir_all(&base);
    let src = base.join("src");
    let dist = base.join("dist");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dist).unwrap();
    (src, dist)
}

fn write_at(dir: &std::path::Path, rel: &str, src: &str) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, src).unwrap();
    path
}

/// Builds `entry` and moves the artifact into an empty directory, so what
/// runs there can only be what the binary carries inside itself.
fn build_and_isolate(entry: &std::path::Path, dist: &std::path::Path, name: &str) -> PathBuf {
    let exe_name = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let built = entry.parent().unwrap().join(&exe_name);
    let build = Command::new(qu_bin())
        .args(["build", entry.to_str().unwrap(), "-o", built.to_str().unwrap()])
        .output()
        .expect("failed to run qu build");
    assert!(
        build.status.success(),
        "qu build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let isolated = dist.join(&exe_name);
    std::fs::copy(&built, &isolated).unwrap();
    isolated
}

fn run_isolated(exe: &std::path::Path) -> String {
    let out = Command::new(exe).output().expect("failed to run the built binary");
    assert!(
        out.status.success(),
        "built binary failed when run away from its sources: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n").trim().to_string()
}

#[test]
fn built_binary_carries_its_imported_modules_when_copied_elsewhere() {
    let (src, dist) = project_dirs("simple");
    write_at(&src, "lib/greet.qu", "function greet(n)\n  return \"hello, \" + n\nend function\n");
    let entry = write_at(&src, "app.qu", "import \"lib/greet.qu\"\nprint(greet(\"world\"))\n");
    let exe = build_and_isolate(&entry, &dist, "app");
    assert_eq!(run_isolated(&exe), "hello, world");
}

#[test]
fn bundling_follows_imports_transitively_through_modules() {
    // `lib/greet.qu` imports `util.qu` relative to ITS OWN directory, not
    // the entry script's -- so a walker that resolved every import against
    // the entry directory would look for `<root>/util.qu` and miss this.
    let (src, dist) = project_dirs("nested");
    write_at(&src, "lib/util.qu", "function shout(s)\n  return upper(s) + \"!\"\nend function\n");
    write_at(
        &src,
        "lib/greet.qu",
        "import \"util.qu\"\nfunction greet(n)\n  return shout(\"hello, \" + n)\nend function\n",
    );
    let entry = write_at(&src, "app.qu", "import \"lib/greet.qu\"\nprint(greet(\"world\"))\n");
    let exe = build_and_isolate(&entry, &dist, "nested");
    assert_eq!(run_isolated(&exe), "HELLO, WORLD!");
}

#[test]
fn bundling_finds_an_import_nested_inside_a_block() {
    // `import` is an ordinary statement, so it can appear inside `if`.
    // This is the case a top-level-only AST walk silently under-bundles,
    // which is why `collect_bundle` scans tokens instead.
    let (src, dist) = project_dirs("conditional");
    write_at(&src, "lib/cond.qu", "function cond_val()\n  return 42\nend function\n");
    let entry = write_at(
        &src,
        "app.qu",
        "if true then\n  import \"lib/cond.qu\"\nend if\nprint(cond_val())\n",
    );
    let exe = build_and_isolate(&entry, &dist, "conditional");
    assert_eq!(run_isolated(&exe), "42");
}

#[test]
fn a_module_imported_by_two_others_is_bundled_and_run_once() {
    // Diamond: the shared module must be stored under one key and, at run
    // time, still honour `import`'s run-once rule through the bundle's own
    // dedup key rather than the filesystem canonicalisation it cannot use.
    let (src, dist) = project_dirs("diamond");
    write_at(&src, "lib/base.qu", "print(\"base ran\")\nfunction base()\n  return 1\nend function\n");
    write_at(&src, "lib/a.qu", "import \"base.qu\"\nfunction av()\n  return base() + 10\nend function\n");
    write_at(&src, "lib/b.qu", "import \"base.qu\"\nfunction bv()\n  return base() + 20\nend function\n");
    let entry = write_at(
        &src,
        "app.qu",
        "import \"lib/a.qu\"\nimport \"lib/b.qu\"\nprint(av() + bv())\n",
    );
    let exe = build_and_isolate(&entry, &dist, "diamond");
    // "base ran" exactly once proves the run-once rule survived the move
    // to bundle keys; 32 proves both importers still see it.
    assert_eq!(run_isolated(&exe), "base ran\n32");
}

#[test]
fn bundling_handles_an_import_from_above_the_entry_directory() {
    // `import "../shared/x.qu"` is an ordinary layout. Its bundle key needs
    // a `..` segment, and build time (relative paths) and run time
    // (absolute, from the exe's own directory) must agree on it.
    let (src, dist) = project_dirs("parentdir");
    write_at(
        src.parent().unwrap(),
        "shared/ext.qu",
        "function ext()\n  return \"from shared\"\nend function\n",
    );
    let entry = write_at(&src, "app.qu", "import \"../shared/ext.qu\"\nprint(ext())\n");
    let exe = build_and_isolate(&entry, &dist, "parentdir");
    // `dist` is a sibling of `src`, so `../shared/ext.qu` resolved from the
    // exe's own directory would ALSO find the real file on disk -- which
    // would pass whether or not bundling worked. Delete the sources first
    // so only the bundle can answer.
    std::fs::remove_dir_all(src.parent().unwrap().join("shared")).unwrap();
    std::fs::remove_dir_all(&src).unwrap();
    assert_eq!(run_isolated(&exe), "from shared");
}

#[test]
fn a_script_with_no_imports_still_builds_and_runs() {
    let (src, dist) = project_dirs("plain");
    let entry = write_at(&src, "app.qu", "print(\"plain\")\n");
    let exe = build_and_isolate(&entry, &dist, "plain");
    assert_eq!(run_isolated(&exe), "plain");
}

#[test]
fn a_missing_imported_file_fails_the_build_instead_of_shipping_a_broken_binary() {
    // The old single-script build could not even see this: it embedded the
    // entry script without reading its imports, so a mistyped module path
    // built green and failed on the user's machine.
    let (src, _dist) = project_dirs("missing");
    let entry = write_at(&src, "app.qu", "import \"nope.qu\"\nprint(1)\n");
    let built = src.join(if cfg!(windows) { "missing.exe" } else { "missing" });
    let build = Command::new(qu_bin())
        .args(["build", entry.to_str().unwrap(), "-o", built.to_str().unwrap()])
        .output()
        .expect("failed to run qu build");
    assert!(!build.status.success(), "build should fail when an imported file is missing");
    assert!(!built.exists(), "a failed build must not leave an artifact behind");
}


#[test]
fn the_build_reports_which_interpreter_it_embedded() {
    // `qu build` copies the RUNNING `qu`, so the artifact silently
    // inherits that binary's optimisation level and compiled-in feature
    // set -- choices the user never made at this call site and cannot see
    // by looking at the result. The build has to say so.
    //
    // Asserted against this test binary's OWN cfg rather than a hardcoded
    // "debug": the spawned `qu` is built with the same profile as the
    // test, so `cargo test` and `cargo test --release` each check their
    // own branch. Hardcoding either one would leave the other untested
    // while still looking green.
    let (src, _dist) = project_dirs("profile");
    let entry = write_at(&src, "app.qu", "print(1)\n");
    let built = src.join(if cfg!(windows) { "profile.exe" } else { "profile" });
    let out = Command::new(qu_bin())
        .args(["build", entry.to_str().unwrap(), "-o", built.to_str().unwrap()])
        .output()
        .expect("failed to run qu build");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let expected_profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    assert!(
        stdout.contains("interpreter:") && stdout.contains(expected_profile),
        "build should report the embedded interpreter profile ({expected_profile}); got: {stdout}"
    );

    // The debug warning goes to STDERR, so a script parsing stdout is
    // unaffected by it -- and it must actually be there when it applies.
    let stderr = String::from_utf8_lossy(&out.stderr);
    if cfg!(debug_assertions) {
        assert!(
            stderr.contains("DEBUG build"),
            "a debug stub must warn that the artifact ships a debug interpreter; got: {stderr}"
        );
    } else {
        assert!(
            !stderr.contains("DEBUG build"),
            "a release stub must not warn about debug; got: {stderr}"
        );
    }
}
