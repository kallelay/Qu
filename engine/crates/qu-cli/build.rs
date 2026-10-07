//! Windows resources for `qu.exe`: version information, an icon and a manifest.
//!
//! Until 0.4.9 the executable carried NO version information at all (empty
//! product name, company, description and file version) and no icon. An
//! anonymous, unsigned, never-seen-before executable is the shape
//! machine-learning antivirus engines distrust most, and Qu's own
//! capabilities (it runs programs and talks to the network, like any
//! scripting language) do not help. Filling these in is cheap and honest; it
//! is NOT a substitute for code signing (see docs/code-signing.md).
//!
//! The manifest only says what the program already does: it runs as the
//! invoking user (never asks for elevation) and works on Windows 10/11.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../../qu-studio-tauri/src-tauri/icons/icon.ico");
    // `cfg!(windows)` in a build script describes the HOST; the target decides.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "Qu")
        .set("FileDescription", "Qu scientific scripting language")
        .set("CompanyName", "Qu project")
        .set("LegalCopyright", "Copyright (c) Qu project contributors. Apache-2.0.")
        .set("OriginalFilename", "qu.exe")
        .set("InternalName", "qu");
    let icon = std::path::Path::new("../../../qu-studio-tauri/src-tauri/icons/icon.ico");
    if icon.exists() {
        res.set_icon(icon.to_str().expect("icon path is valid UTF-8"));
    }
    res.set_manifest(
        r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>"#,
    );
    // A failure to attach resources must not break a build on a machine
    // without a resource compiler: say so and carry on without them.
    if let Err(e) = res.compile() {
        println!("cargo:warning=could not embed Windows version resources: {e}");
    }
}
