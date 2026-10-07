# Editor integrations for Qu

`qu editors` (built into the `qu` binary, which embeds every file here)
detects, installs, checks and removes these:

| Editor | Folder | Installed to |
|---|---|---|
| VS Code, VSCodium, Cursor, Windsurf | `vscode-qu/` | `<home>/.vscode\|.vscode-oss\|.cursor\|.windsurf/extensions/qu-project.qu-language-<ver>/` (portable: `data/extensions`; Flatpak: `~/.var/app/...`) |
| Notepad++ (Windows) | `notepadpp-qu/` | `%APPDATA%\Notepad++\userDefineLangs\Qu.udl.xml` (portable: next to `notepad++.exe`) |
| Sublime Text 3 and 4 | `sublime-qu/` | `Packages/User/Qu/` |
| Vim | `vim-qu/` | `~/.vim/pack/qu/start/qu/` (Windows `vimfiles`) |
| Neovim | `vim-qu/` | `~/.config/nvim/pack/qu/start/qu/` (Windows `%LOCALAPPDATA%\nvim`) |
| Jupyter (JupyterLab, Notebook, VS Code notebooks) | `engine/crates/qu-jupyter` | kernelspec `qu` via the sibling `qu-jupyter` binary |

```
qu editors detect [<editor>] [--json]     where each editor is and where its plugin goes (exit 0/1 with a name)
qu editors status [--json]                installed? version? files intact? qu on PATH? does the CLI list it?
qu editors install [--editor auto|all|NAME[,NAME]] [--root DIR] [--dry-run]
qu editors uninstall [--editor ...] [--root DIR] [--dry-run]
qu editors check [--json]                 exit 0 only if everything installed is healthy
```

Detection order: Windows registry (`App Paths`, then the `Uninstall` keys, both
registry views, through `reg.exe`), `PATH`, known folders (Program Files,
`%LOCALAPPDATA%\Programs`); macOS `/Applications` and `~/Applications`; Linux
`PATH`, `/usr`, `/opt`, `/snap`, Flatpak. Each result says which method found
it. Editors in custom folders are found through the registry or `PATH`.

`--editor auto` (the default for `install`) installs into every detected
editor; `all` writes to every known default location even if the editor is
not detected. Safe to rerun; user-modified files are saved as `.bak`.

## Not yet supported

| Editor | Why |
|---|---|
| JetBrains IDEs | They can import the TextMate grammar `vscode-qu/syntaxes/qu.tmLanguage.json` as a TextMate bundle (Settings > Editor > TextMate Bundles); no installer yet. |
| Zed, Helix, Neovim tree-sitter | Need a tree-sitter grammar for Qu, which does not exist yet. |
| Emacs | Needs a `qu-mode.el`. |
| Kate | Needs a KSyntaxHighlighting XML definition. |
