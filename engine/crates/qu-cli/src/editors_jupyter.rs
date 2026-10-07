//! The Jupyter side of `qu editors`: find the `qu` kernelspec, judge whether it
//! can really start, and install/uninstall it by running the sibling
//! `qu-jupyter` binary's own `install`/`uninstall` (the JSON is written by that
//! binary, not re-implemented here).

use crate::editors_detect::{which, Env, Platform};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Captured {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Run a program with stdin closed, capture its output, and kill THE CHILD WE
/// SPAWNED (only it) if it is still running after `timeout`.
pub fn run_timeout(
    program: &OsStr,
    args: &[String],
    envs: &[(String, String)],
    timeout: Duration,
) -> Result<Captured, String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", Path::new(program).display()))?;
    let start = Instant::now();
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(e) => return Err(format!("waiting for {}: {e}", Path::new(program).display())),
        }
    }
    let out = child.wait_with_output().map_err(|e| format!("reading output: {e}"))?;
    Ok(Captured {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        timed_out,
    })
}

/// `py -m jupyter` / `python -m jupyter` fallback for detection. Skips the
/// Microsoft Store stub in `WindowsApps` (running it opens the Store).
pub fn python_m_jupyter(env: &Env) -> Option<(PathBuf, String)> {
    for name in ["py", "python", "python3"] {
        let Some(p) = which(env, name) else { continue };
        if p.to_string_lossy().contains("WindowsApps") {
            continue;
        }
        let r = run_timeout(
            p.as_os_str(),
            &["-m".into(), "jupyter".into(), "--version".into()],
            &[],
            Duration::from_secs(10),
        );
        if let Ok(c) = r {
            if !c.timed_out && c.code == Some(0) {
                return Some((p, format!("{name} -m jupyter")));
            }
        }
    }
    None
}

pub struct KernelSpec {
    pub dir: PathBuf,
    pub json_path: PathBuf,
}

/// The kernelspec named `qu` on Jupyter's search path (user dir first).
pub fn find_kernelspec(env: &Env) -> Option<KernelSpec> {
    for root in env.jupyter_kernel_roots() {
        let dir = root.join("kernels").join("qu");
        let json_path = dir.join("kernel.json");
        if json_path.is_file() {
            return Some(KernelSpec { dir, json_path });
        }
    }
    None
}

pub struct KernelHealth {
    pub dir: PathBuf,
    pub argv0: Option<String>,
    pub problems: Vec<String>,
    pub notes: Vec<String>,
}

fn is_executable(env: &Env, p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = env;
        std::fs::metadata(p).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = env;
        let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        matches!(ext.as_str(), "exe" | "cmd" | "bat" | "com")
    }
}

/// Judge the `qu` kernelspec. `smoke`: also start the kernel with a missing
/// connection file and check it fails fast instead of hanging.
pub fn kernel_health(env: &Env, smoke: bool) -> Option<KernelHealth> {
    let ks = find_kernelspec(env)?;
    let mut h = KernelHealth { dir: ks.dir.clone(), argv0: None, problems: Vec::new(), notes: Vec::new() };
    let text = match std::fs::read_to_string(&ks.json_path) {
        Ok(t) => t,
        Err(e) => {
            h.problems.push(format!("cannot read {}: {e}", ks.json_path.display()));
            return Some(h);
        }
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            h.problems.push(format!("kernel.json is not valid JSON: {e}"));
            return Some(h);
        }
    };
    let argv: Vec<String> = match v.get("argv").and_then(|a| a.as_array()) {
        Some(a) if !a.is_empty() && a.iter().all(|x| x.is_string()) => {
            a.iter().map(|x| x.as_str().unwrap().to_string()).collect()
        }
        _ => {
            h.problems.push("kernel.json has no usable \"argv\" array of strings".into());
            return Some(h);
        }
    };
    h.argv0 = Some(argv[0].clone());
    let exe = PathBuf::from(&argv[0]);
    if !exe.is_file() {
        h.problems.push(format!("argv[0] does not exist on disk: {}", argv[0]));
    } else if !is_executable(env, &exe) {
        h.problems.push(format!("argv[0] is not executable: {}", argv[0]));
    }
    if !argv.iter().any(|a| a == "{connection_file}") {
        h.problems.push("argv does not contain {connection_file}: Jupyter cannot hand the kernel its ports".into());
    }
    if v.get("language").and_then(|l| l.as_str()) != Some("qu") {
        h.notes.push("kernel.json \"language\" is not \"qu\"".into());
    }
    if smoke && h.problems.is_empty() {
        let missing = std::env::temp_dir().join(format!("qu-editors-smoke-{}.json", std::process::id()));
        let args: Vec<String> = argv[1..]
            .iter()
            .map(|a| if a == "{connection_file}" { missing.to_string_lossy().into_owned() } else { a.clone() })
            .collect();
        match run_timeout(exe.as_os_str(), &args, &[], Duration::from_secs(5)) {
            Err(e) => h.problems.push(format!("smoke test: {e}")),
            Ok(c) if c.timed_out => h.problems.push(
                "smoke test: the kernel did not exit within 5 s when given a missing connection file (it hangs instead of failing); killed it"
                    .into(),
            ),
            Ok(c) => {
                let first = c.stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                match c.code {
                    Some(0) => h.notes.push("smoke test: kernel exited 0 with a missing connection file (expected an error)".into()),
                    Some(n) if first.is_empty() => {
                        h.notes.push(format!("smoke test: failed fast (exit {n}) but printed no message"))
                    }
                    Some(n) => h.notes.push(format!("smoke test: fails fast on a missing connection file (exit {n}: {first})")),
                    None => h.notes.push("smoke test: kernel was terminated by a signal".into()),
                }
            }
        }
    }
    Some(h)
}

/// Where `qu-jupyter` should be: `QU_EDITORS_JUPYTER_BIN`, else next to `qu`.
pub fn qu_jupyter_bin(env: &Env) -> PathBuf {
    if let Some(p) = std::env::var_os("QU_EDITORS_JUPYTER_BIN") {
        return PathBuf::from(p);
    }
    let name = if env.platform == Platform::Windows { "qu-jupyter.exe" } else { "qu-jupyter" };
    match env.qu_exe.as_ref().and_then(|p| p.parent()) {
        Some(d) => d.join(name),
        None => PathBuf::from(name),
    }
}

fn child_env(env: &Env) -> Vec<(String, String)> {
    vec![("JUPYTER_DATA_DIR".to_string(), env.jupyter_user_dir().to_string_lossy().into_owned())]
}

pub fn install(env: &Env, dry: bool) -> Result<Vec<String>, String> {
    let bin = qu_jupyter_bin(env);
    let mut log = Vec::new();
    if !bin.is_file() {
        return Err(format!(
            "qu-jupyter not found next to qu ({}). It ships in the release zip next to qu.exe; to build it: cargo build --release -p qu-jupyter",
            bin.display()
        ));
    }
    let kj = env.jupyter_user_dir().join("kernels").join("qu").join("kernel.json");
    if dry {
        log.push(format!("would run {} install (JUPYTER_DATA_DIR={})", bin.display(), env.jupyter_user_dir().display()));
        log.push(format!("would write {}", kj.display()));
        return Ok(log);
    }
    let before = std::fs::read(&kj).ok();
    let c = run_timeout(bin.as_os_str(), &["install".into()], &child_env(env), Duration::from_secs(30))?;
    if c.timed_out || c.code != Some(0) {
        return Err(format!("qu-jupyter install failed (exit {:?}): {}", c.code, c.stderr.trim()));
    }
    if let (Some(old), Ok(new)) = (before, std::fs::read(&kj)) {
        if old != new {
            let bak = kj.with_extension("json.bak");
            if !bak.exists() {
                let _ = std::fs::write(&bak, old);
                log.push(format!("previous kernel.json saved as {}", bak.display()));
            }
        }
    }
    log.push(format!("kernelspec written: {}", kj.display()));
    log.push(format!("via {}", bin.display()));
    Ok(log)
}

pub fn uninstall(env: &Env, dry: bool) -> Result<Vec<String>, String> {
    let kernels = env.jupyter_user_dir().join("kernels").join("qu");
    let kj = kernels.join("kernel.json");
    if !kj.is_file() {
        return Ok(vec![format!("no qu kernelspec at {}", kernels.display())]);
    }
    if dry {
        return Ok(vec![format!("would remove {}", kj.display())]);
    }
    let bin = qu_jupyter_bin(env);
    if bin.is_file() {
        let c = run_timeout(bin.as_os_str(), &["uninstall".into()], &child_env(env), Duration::from_secs(30))?;
        if c.code == Some(0) {
            return Ok(vec![format!("removed the qu kernelspec from {}", kernels.display())]);
        }
    }
    // The binary is gone: do exactly what its uninstall does.
    std::fs::remove_file(&kj).map_err(|e| format!("removing {}: {e}", kj.display()))?;
    let _ = std::fs::remove_dir(&kernels);
    Ok(vec![format!("removed {}", kj.display())])
}
