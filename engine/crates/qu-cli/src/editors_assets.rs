//! The editor plugin files, embedded in the binary.
//!
//! `qu editors install` must work from a bare `qu.exe` out of the release
//! zip, with no `editors/` folder beside it, so every plugin file is baked in
//! with `include_str!`. The paths are relative to THIS file
//! (`engine/crates/qu-cli/src/`), four levels up to the repository root.

pub struct Asset {
    /// Path relative to the plugin's install root.
    pub rel: &'static str,
    pub data: &'static str,
}

pub const VSCODE_ID: &str = "qu-project.qu-language";
pub const VSCODE_FILES: &[Asset] = &[
    Asset { rel: "package.json", data: include_str!("../../../../editors/vscode-qu/package.json") },
    Asset { rel: "extension.js", data: include_str!("../../../../editors/vscode-qu/extension.js") },
    Asset {
        rel: "language-configuration.json",
        data: include_str!("../../../../editors/vscode-qu/language-configuration.json"),
    },
    Asset {
        rel: "syntaxes/qu.tmLanguage.json",
        data: include_str!("../../../../editors/vscode-qu/syntaxes/qu.tmLanguage.json"),
    },
    Asset {
        rel: "scripts/qu_htmldiff.qu",
        data: include_str!("../../../../editors/vscode-qu/scripts/qu_htmldiff.qu"),
    },
    Asset { rel: "README.md", data: include_str!("../../../../editors/vscode-qu/README.md") },
    Asset { rel: "CHANGELOG.md", data: include_str!("../../../../editors/vscode-qu/CHANGELOG.md") },
];

pub const NOTEPADPP_FILES: &[Asset] = &[Asset {
    rel: "Qu.udl.xml",
    data: include_str!("../../../../editors/notepadpp-qu/Qu.udl.xml"),
}];

pub const SUBLIME_FILES: &[Asset] = &[
    Asset { rel: "Qu.sublime-syntax", data: include_str!("../../../../editors/sublime-qu/Qu.sublime-syntax") },
    Asset { rel: "Qu.sublime-build", data: include_str!("../../../../editors/sublime-qu/Qu.sublime-build") },
];

pub const VIM_FILES: &[Asset] = &[
    Asset { rel: "syntax/qu.vim", data: include_str!("../../../../editors/vim-qu/syntax/qu.vim") },
    Asset { rel: "ftdetect/qu.vim", data: include_str!("../../../../editors/vim-qu/ftdetect/qu.vim") },
    Asset { rel: "ftplugin/qu.vim", data: include_str!("../../../../editors/vim-qu/ftplugin/qu.vim") },
];

/// FNV-1a 64 over the bytes, as 16 hex digits. Not a security hash: it only
/// answers "is this file byte-for-byte what we wrote".
pub fn fnv64_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// The plugin version a file carries: the first `Qu editor plugin X.Y.Z`
/// marker in it (every embedded file has one; the VS Code package.json has a
/// real `version` field instead, see [`vscode_version`]).
pub fn marker_version(text: &str) -> Option<String> {
    let i = text.find("Qu editor plugin ")?;
    let rest = &text[i + "Qu editor plugin ".len()..];
    let v: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// The version of the embedded VS Code extension (from its package.json).
pub fn vscode_version() -> String {
    let pkg = VSCODE_FILES.iter().find(|a| a.rel == "package.json").map(|a| a.data).unwrap_or("{}");
    serde_json::from_str::<serde_json::Value>(pkg)
        .ok()
        .and_then(|v| v.get("version").and_then(|s| s.as_str().map(String::from)))
        .unwrap_or_else(|| "0.0.0".to_string())
}

/// Version the embedded plugin of this kind ships.
pub fn shipped_version(kind: PluginKind) -> String {
    match kind {
        PluginKind::VsCode => vscode_version(),
        PluginKind::NotepadPp => marker_version(NOTEPADPP_FILES[0].data).unwrap_or_default(),
        PluginKind::Sublime => marker_version(SUBLIME_FILES[0].data).unwrap_or_default(),
        PluginKind::Vim => marker_version(VIM_FILES[0].data).unwrap_or_default(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PluginKind {
    VsCode,
    NotepadPp,
    Sublime,
    Vim,
}

pub fn files_for(kind: PluginKind) -> &'static [Asset] {
    match kind {
        PluginKind::VsCode => VSCODE_FILES,
        PluginKind::NotepadPp => NOTEPADPP_FILES,
        PluginKind::Sublime => SUBLIME_FILES,
        PluginKind::Vim => VIM_FILES,
    }
}
