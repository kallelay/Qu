//! `qu-jupyter install` — writes a Jupyter kernelspec pointing at this exact
//! binary, so both Jupyter (JupyterLab/Notebook/`jupyter console`) and VS
//! Code's Jupyter extension can find and launch it. Both discover kernels
//! the same way (scanning the standard `.../jupyter/kernels/<name>/kernel.json`
//! directories), so one install step covers both hosts — there's no
//! separate VS Code extension to write.

use std::path::PathBuf;

pub struct InstallOptions {
    /// Install under the user's own data dir (default) rather than a
    /// `--prefix`. Kept even though it's currently the only supported mode
    /// — `--prefix` below is the escape hatch for anything else (a venv, a
    /// shared install) without needing a second code path later.
    pub prefix: Option<PathBuf>,
}

pub fn run(opts: InstallOptions) -> Result<(), String> {
    // Deliberately NOT `.canonicalize()`: on Windows that prepends the `\\?\`
    // verbatim-path prefix, which `current_exe()` alone never produces (it's
    // already absolute) and which some tools that shell out to `argv[0]`
    // (including, empirically, VS Code's Jupyter extension) mishandle.
    let exe = std::env::current_exe().map_err(|e| format!("locating this executable: {e}"))?;

    let kernels_dir = match &opts.prefix {
        Some(prefix) => prefix.join("share").join("jupyter").join("kernels"),
        None => user_jupyter_data_dir()?.join("kernels"),
    };
    let dest = kernels_dir.join("qu");
    std::fs::create_dir_all(&dest).map_err(|e| format!("creating {}: {e}", dest.display()))?;

    let kernel_json = serde_json::json!({
        "argv": [exe.to_string_lossy(), "-f", "{connection_file}"],
        "display_name": "Qu",
        "language": "qu",
        "interrupt_mode": "message",
    });
    let kernel_json_path = dest.join("kernel.json");
    std::fs::write(
        &kernel_json_path,
        serde_json::to_string_pretty(&kernel_json).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("writing {}: {e}", kernel_json_path.display()))?;

    println!("Installed Qu kernelspec at {}", dest.display());
    println!("  argv[0] = {}", exe.display());
    println!();
    println!("Jupyter (JupyterLab / Notebook / jupyter console): \"Qu\" now appears in the kernel picker.");
    println!("VS Code: open or create a .ipynb file, use \"Select Kernel\" -> \"Jupyter Kernel...\" -> \"Qu\"");
    println!("(requires Microsoft's \"Jupyter\" extension, which discovers kernels from the same");
    println!("directory Jupyter itself uses — no separate Qu extension is needed).");
    Ok(())
}

/// Jupyter's own documented per-platform data directories
/// (`jupyter --paths`), reimplemented directly rather than shelling out to
/// a `jupyter` binary — this installer should work even on a machine where
/// only this Rust binary exists yet, before any Python/Jupyter environment
/// is set up. `$JUPYTER_DATA_DIR` overrides on every platform, matching
/// Jupyter's own precedence.
fn user_jupyter_data_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("JUPYTER_DATA_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").map_err(|_| "%APPDATA% is not set".to_string())?;
        Ok(PathBuf::from(appdata).join("jupyter"))
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").map_err(|_| "$HOME is not set".to_string())?;
        Ok(PathBuf::from(home).join("Library").join("Jupyter"))
    } else {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.is_empty() {
                return Ok(PathBuf::from(xdg).join("jupyter"));
            }
        }
        let home = std::env::var("HOME").map_err(|_| "$HOME is not set".to_string())?;
        Ok(PathBuf::from(home).join(".local").join("share").join("jupyter"))
    }
}
