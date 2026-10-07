; Installer for the `qu` command-line binary on Windows.
;
; Build (CI does this in .github/workflows/release.yml):
;   makensis /DVERSION=0.4.4 /DARCH=x86_64 /DSRCDIR=<dir with qu.exe> ^
;            [/DOUTDIR=<output dir>] installer\windows\qu.nsi
; Produces qu-<VERSION>-windows-<ARCH>-setup.exe. ARCH is x86_64 or arm64.
;
; Optional components (Components page; defaults in brackets), each built
; only when its files are in SRCDIR -- the release stages all of them:
;   Jupyter kernel [on]        SRCDIR\qu-jupyter.exe; registers the "Qu"
;                              kernelspec (qu-jupyter install [--system])
;   Offline documentation [off] SRCDIR\docs\ (the website, ~25 MB)
;   Editor plugins             SRCDIR\editors\; VS Code, Sublime Text,
;                              Notepad++ -- each on only if that editor's
;                              config folder exists
;   .qu file type [on]         "Qu script" type + icon; double-click OPENS
;                              the file in an editor (never runs it); a
;                              right-click "Run with Qu" verb runs it
;                              (see ASSOC_WRITE); /NOASSOC skips
;   Start menu shortcuts [on]  Qu CLI (REPL), Start Jupyter (Qu), Qu
;                              Documentation, Uninstall
;   Desktop shortcuts [off]
;
; Runtime switches:
;   /S           silent
;   /WITHDOCS /NOJUPYTER /NOPLUGINS /NOSHORTCUTS /DESKTOP /NOPATH /NOASSOC
;                choose components without the page (for /S installs)
;   /D=<dir>     install directory (must be last, NSIS rule)
;   /ALLUSERS    machine-wide: Program Files, HKLM PATH, HKLM uninstall
;                entry; relaunches itself elevated if needed
; Default is per-user: %LOCALAPPDATA%\Programs\Qu, HKCU PATH, no UAC.
;
; PATH is edited by path-helper.ps1 (installed next to qu.exe), never with
; NSIS string code: NSIS strings stop at 1024 characters and a longer PATH
; would be truncated and written back. See that file for the rules.
;
; This installer is exercised for real by
; .github/workflows/installer-verify-cli.yml; do not test it by running it
; on a developer machine -- it edits that machine's PATH.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma

!ifndef VERSION
  !error "pass /DVERSION=<x.y.z>"
!endif
!ifndef ARCH
  !error "pass /DARCH=x86_64 or /DARCH=arm64"
!endif
!if "${ARCH}" != "x86_64"
!if "${ARCH}" != "arm64"
  !error "ARCH must be x86_64 or arm64, got '${ARCH}'"
!endif
!endif
!ifndef SRCDIR
  !error "pass /DSRCDIR=<directory holding qu.exe, LICENSE, NOTICE, README.md>"
!endif
!ifndef OUTDIR
  !define OUTDIR "."
!endif

; VIProductVersion needs four numbers; "0.4.4-rc1" -> 0.4.4.0, "dev" -> 0.0.0.0.
!searchparse /noerrors "${VERSION}-" "" VMAJ "." VMIN "." VPAT "-"
; A partial match (e.g. "dev" sets VMAJ to "dev-" and nothing else) is
; discarded whole. A non-numeric x.y.z still fails the build, loudly.
!ifndef VPAT
  !ifdef VMAJ
    !undef VMAJ
  !endif
  !ifdef VMIN
    !undef VMIN
  !endif
  !define VMAJ 0
  !define VMIN 0
  !define VPAT 0
!endif

!define PRODUCT      "Qu"
!define PUBLISHER    "Ahmed Yahia Kallel"
!define URL          "https://github.com/kallelay/Qu"
; Distinct from Qu Studio's own uninstall key; the two install separately.
!define UNINST_KEY   "Software\Microsoft\Windows\CurrentVersion\Uninstall\QuCLI"
!define STATE_FILE   "install.ini"

Name "${PRODUCT} ${VERSION} (command line)"
OutFile "${OUTDIR}\qu-${VERSION}-windows-${ARCH}-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Qu"
RequestExecutionLevel user
ShowInstDetails show
ShowUninstDetails show
BrandingText "${PRODUCT} ${VERSION}"

VIProductVersion "${VMAJ}.${VMIN}.${VPAT}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCT} ${VERSION} command-line installer (${ARCH})"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "(c) 2026 ${PUBLISHER}"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "WinMessages.nsh"
!include "x64.nsh"
!include "Sections.nsh"

; What this build can offer (see the header).
!if /FileExists "${SRCDIR}\qu-jupyter.exe"
  !define HAVE_JUPYTER
!endif
!if /FileExists "${SRCDIR}\docs\index.html"
  !define HAVE_DOCS
!endif
!if /FileExists "${SRCDIR}\editors\vscode-qu\package.json"
  !define HAVE_EDITORS
  ; the folder name VS Code expects: <publisher-less name>-<version>
  !searchparse /file "${SRCDIR}\editors\vscode-qu\package.json" `"version": "` VSCODE_EXT_VER `"`
!endif
!define SM_DIR "$SMPROGRAMS\Qu"

Var AllUsers     ; 1 = machine-wide
Var PathAdded    ; 1 = this install owns a PATH entry (persisted in install.ini)
Var PsExe

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_TEXT "Qu is installed. If you kept $\"Add qu to the PATH$\", open a new terminal and run:$\r$\n$\r$\n    qu --version"
!insertmacro MUI_PAGE_LICENSE "${SRCDIR}\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; ---- shared helpers (installer and uninstaller) ------------------------

!macro FIND_POWERSHELL
  ; 64-bit PowerShell from this 32-bit installer: SysNative bypasses the
  ; WOW64 System32 redirect. Falls back to whatever System32 resolves to.
  ${If} ${FileExists} "$WINDIR\SysNative\WindowsPowerShell\v1.0\powershell.exe"
    StrCpy $PsExe "$WINDIR\SysNative\WindowsPowerShell\v1.0\powershell.exe"
  ${Else}
    StrCpy $PsExe "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  ${EndIf}
!macroend

; Leaves the helper's exit code in $0 (0 changed, 10 no-op, else error).
!macro RUN_PATH_HELPER ACTION
  !insertmacro FIND_POWERSHELL
  ${If} $AllUsers == 1
    StrCpy $1 "Machine"
  ${Else}
    StrCpy $1 "User"
  ${EndIf}
  nsExec::ExecToLog '"$PsExe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\path-helper.ps1" -Action ${ACTION} -Dir "$INSTDIR" -Scope $1'
  Pop $0
!macroend

!macro SET_SCOPE_CONTEXT
  ${If} $AllUsers == 1
    SetShellVarContext all
    SetRegView 64
  ${Else}
    SetShellVarContext current
  ${EndIf}
!macroend

; Relaunch this same executable elevated with the same arguments plus
; /ELEVATED (loop guard), wait for it, and quit. LASTOPT is the trailing
; "/D=" (installer) or "_?=" (uninstaller) option, which NSIS requires last
; and may already have cut out of $CMDLINE; it is re-appended as LASTOPT
; LASTVAL unless the parameters still carry it. The elevated child's exit
; code is not propagated (ExecShellWait does not return one).
!macro RELAUNCH_ELEVATED LASTOPT LASTVAL
  UserInfo::GetAccountType
  Pop $0
  ${If} $0 != "admin"
    ${GetParameters} $1
    ClearErrors
    ${GetOptions} $1 "/ELEVATED" $2
    ${IfNot} ${Errors}
      MessageBox MB_ICONSTOP "Administrator rights are needed for /ALLUSERS and were not granted." /SD IDOK
      SetErrorLevel 740
      Quit
    ${EndIf}
    StrCpy $3 "/ELEVATED $1"
    ClearErrors
    ${GetOptions} $1 "${LASTOPT}" $2
    ${If} ${Errors}
    ${AndIf} "${LASTVAL}" != ""
      StrCpy $3 "$3 ${LASTOPT}${LASTVAL}"
    ${EndIf}
    ClearErrors
    ExecShellWait "runas" "$EXEPATH" "$3"
    ${If} ${Errors}
      SetErrorLevel 740
    ${EndIf}
    Quit
  ${EndIf}
!macroend

; Add/Remove Programs entry. $R1 is "" or " /ALLUSERS", $R2 the size in KB.
!macro WRITE_ARP ROOT
  WriteRegStr   ${ROOT} "${UNINST_KEY}" "DisplayName"          "Qu ${VERSION} (command line, ${ARCH})"
  WriteRegStr   ${ROOT} "${UNINST_KEY}" "DisplayVersion"       "${VERSION}"
  WriteRegStr   ${ROOT} "${UNINST_KEY}" "Publisher"            "${PUBLISHER}"
  WriteRegStr   ${ROOT} "${UNINST_KEY}" "URLInfoAbout"         "${URL}"
  WriteRegStr   ${ROOT} "${UNINST_KEY}" "DisplayIcon"          "$INSTDIR\qu.exe"
  WriteRegStr   ${ROOT} "${UNINST_KEY}" "InstallLocation"      "$INSTDIR"
  WriteRegStr   ${ROOT} "${UNINST_KEY}" "UninstallString"      '"$INSTDIR\uninstall.exe"$R1'
  WriteRegStr   ${ROOT} "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S$R1'
  WriteRegDWORD ${ROOT} "${UNINST_KEY}" "NoModify"             1
  WriteRegDWORD ${ROOT} "${UNINST_KEY}" "NoRepair"             1
  WriteRegDWORD ${ROOT} "${UNINST_KEY}" "EstimatedSize"        $R2
!macroend

; .qu file association. SAFE BY DEFAULT: double-clicking a .qu file must
; never execute it (a script can do anything the user can), so the default
; verb "open" opens it in a text editor -- Notepad here; installing Qu
; Studio replaces it with Qu Studio. Running is a separate, explicit
; right-click verb, "Run with Qu" (qu run "<file>" in a console that stays
; open). Same ProgID (Qu.Script) as the Studio installer, which owns the
; `open`/`edit` verbs; this installer owns `run`, and writes `open` only
; if none exists. Each uninstaller removes only the verbs that still point
; at its own install, and the ProgID, the OpenWithProgids value and a
; dangling .qu default only when nothing of the other installer is left.
; The `.qu` default is set only if nothing owns .qu (no default value and,
; per user, no Explorer UserChoice): no hijacking. /NOASSOC skips all.
; ROOT is HKCU, or HKLM with /ALLUSERS.
!define PROGID_KEY "Software\Classes\Qu.Script"

!macro ASSOC_WRITE ROOT
  ReadRegStr $R0 ${ROOT} "${PROGID_KEY}\shell\open\command" ""
  ${If} $R0 == ""
    WriteRegStr ${ROOT} "${PROGID_KEY}" "" "Qu script"
    WriteRegStr ${ROOT} "${PROGID_KEY}" "FriendlyTypeName" "Qu script"
    WriteRegStr ${ROOT} "${PROGID_KEY}\DefaultIcon" "" '"$INSTDIR\qu.exe",0'
    WriteRegStr ${ROOT} "${PROGID_KEY}\shell" "" "open"
    WriteRegStr ${ROOT} "${PROGID_KEY}\shell\open" "" "Open"
    WriteRegStr ${ROOT} "${PROGID_KEY}\shell\open\command" "" '"$WINDIR\notepad.exe" "%1"'
  ${EndIf}
  WriteRegStr ${ROOT} "${PROGID_KEY}\shell\run" "" "Run with Qu"
  WriteRegStr ${ROOT} "${PROGID_KEY}\shell\run" "Icon" '"$INSTDIR\qu.exe",0'
  WriteRegStr ${ROOT} "${PROGID_KEY}\shell\run\command" "" '"$SYSDIR\cmd.exe" /k ""$INSTDIR\qu.exe" run "%1""'
  WriteRegStr ${ROOT} "Software\Classes\.qu\OpenWithProgids" "Qu.Script" ""
  ReadRegStr $R0 ${ROOT} "Software\Classes\.qu" ""
  ReadRegStr $R1 HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.qu\UserChoice" "ProgId"
  ${If} $R0 == ""
  ${AndIf} $R1 == ""
    WriteRegStr ${ROOT} "Software\Classes\.qu" "" "Qu.Script"
    DetailPrint ".qu now opens in a text editor (Run with Qu is a right-click verb)"
  ${Else}
    DetailPrint ".qu is already owned by '$R0$R1'; default left alone"
  ${EndIf}
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Assoc" "Run" 1
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)'
!macroend

!macro ASSOC_REMOVE ROOT
  ReadINIStr $R0 "$INSTDIR\${STATE_FILE}" "Assoc" "Run"
  ${If} $R0 == 1
    ReadRegStr $R1 ${ROOT} "${PROGID_KEY}\shell\run\command" ""
    ${If} $R1 == '"$SYSDIR\cmd.exe" /k ""$INSTDIR\qu.exe" run "%1""'
      DeleteRegKey ${ROOT} "${PROGID_KEY}\shell\run"
    ${EndIf}
    ; ProgID still ours alone (Notepad open verb, nothing else): remove it
    ReadRegStr $R1 ${ROOT} "${PROGID_KEY}\shell\open\command" ""
    ${If} $R1 == '"$WINDIR\notepad.exe" "%1"'
      DeleteRegKey ${ROOT} "${PROGID_KEY}"
    ${EndIf}
    ReadRegStr $R1 ${ROOT} "${PROGID_KEY}" ""
    ${If} $R1 == ""
      DeleteRegValue ${ROOT} "Software\Classes\.qu\OpenWithProgids" "Qu.Script"
      DeleteRegKey /ifempty ${ROOT} "Software\Classes\.qu\OpenWithProgids"
      ReadRegStr $R1 ${ROOT} "Software\Classes\.qu" ""
      ${If} $R1 == "Qu.Script"
        DeleteRegValue ${ROOT} "Software\Classes\.qu" ""
      ${EndIf}
      DeleteRegKey /ifempty ${ROOT} "Software\Classes\.qu"
    ${EndIf}
    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)'
  ${EndIf}
!macroend

; Editor plugins: `qu editors install --editor <name>` (the engine finds the
; editor's real data directory: App Paths / Uninstall keys, PATH, known
; folders) first; the plain file copy of the section is the fallback when
; qu.exe lacks the subcommand or cannot find the editor, so this never
; fails the install. Only DetailPrint. $R7 = 0 when the CLI did the work.
!macro EDITOR_VIA_CLI NAME
  StrCpy $R7 "skipped"
  ${If} ${FileExists} "$INSTDIR\qu.exe"
    nsExec::ExecToLog '"$INSTDIR\qu.exe" editors install --editor ${NAME}'
    Pop $R7
    DetailPrint "qu editors install --editor ${NAME}: exit code $R7 (non-zero: falling back to a file copy)"
  ${EndIf}
!macroend

; Component default from the machine: App Paths (HKCU then HKLM: the
; editor's own installer wrote it, wherever it was put) or a config folder;
; the found location shows in the component name.
!macro DETECT_EDITOR SEC LABEL EXE DIR1 DIR2
  ReadRegStr $R2 HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\${EXE}" ""
  ${If} $R2 == ""
    ReadRegStr $R2 HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\${EXE}" ""
  ${EndIf}
  ${If} $R2 != ""
    SectionSetText ${SEC} "${LABEL} ($R2)"
  ${Else}
    ${IfNot} ${FileExists} "${DIR1}\*.*"
    ${AndIfNot} ${FileExists} "${DIR2}\*.*"
      !insertmacro UnselectSection ${SEC}
    ${EndIf}
  ${EndIf}
!macroend

; ---- installer ----------------------------------------------------------

Function .onInit
  !if "${ARCH}" == "arm64"
    ${IfNot} ${IsNativeARM64}
      MessageBox MB_ICONSTOP "This is the Windows on ARM (arm64) build of Qu. Use the x86_64 installer on this machine." /SD IDOK
      SetErrorLevel 2
      Quit
    ${EndIf}
  !else
    ${IfNot} ${IsNativeAMD64}
    ${AndIfNot} ${IsNativeARM64}
      MessageBox MB_ICONSTOP "Qu needs 64-bit Windows." /SD IDOK
      SetErrorLevel 2
      Quit
    ${EndIf}
  !endif

  StrCpy $AllUsers 0
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/ALLUSERS" $R1
  ${IfNot} ${Errors}
    StrCpy $AllUsers 1
    ; Forward /D= only if the user gave one; otherwise let the elevated
    ; copy pick the all-users default itself.
    ${If} $INSTDIR != "$LOCALAPPDATA\Programs\Qu"
      StrCpy $R3 $INSTDIR
    ${Else}
      StrCpy $R3 ""
    ${EndIf}
    !insertmacro RELAUNCH_ELEVATED "/D=" $R3
  ${EndIf}
  !insertmacro SET_SCOPE_CONTEXT

  ; $INSTDIR still equal to the InstallDir default means no /D= was given:
  ; reuse a previous install's location, else the scope's default.
  ${If} $INSTDIR == "$LOCALAPPDATA\Programs\Qu"
    ${If} $AllUsers == 1
      ReadRegStr $R2 HKLM "${UNINST_KEY}" "InstallLocation"
      StrCpy $INSTDIR "$PROGRAMFILES64\Qu"
    ${Else}
      ReadRegStr $R2 HKCU "${UNINST_KEY}" "InstallLocation"
    ${EndIf}
    ${If} $R2 != ""
      StrCpy $INSTDIR $R2
    ${EndIf}
  ${EndIf}

  Call QuComponentDefaults
FunctionEnd

Section "Qu command line (required)" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${SRCDIR}\qu.exe"
  File "${SRCDIR}\LICENSE"
  File "${SRCDIR}\NOTICE"
  File "${SRCDIR}\README.md"
  File "path-helper.ps1"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; A reinstall over an install that added the PATH entry finds it already
  ; there (helper returns 10); keep ownership so uninstall still removes it.
  ReadINIStr $PathAdded "$INSTDIR\${STATE_FILE}" "Install" "PathAdded"
  ${If} $PathAdded != 1
    StrCpy $PathAdded 0
  ${EndIf}
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "PathAdded" $PathAdded
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "AllUsers" $AllUsers
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "Version" "${VERSION}"

  ${GetSize} "$INSTDIR" "/S=0K" $R2 $R3 $R4
  ${If} $AllUsers == 1
    StrCpy $R1 " /ALLUSERS"
    !insertmacro WRITE_ARP HKLM
  ${Else}
    StrCpy $R1 ""
    !insertmacro WRITE_ARP HKCU
  ${EndIf}
SectionEnd

; On by default; unticked (or /NOPATH) leaves PATH alone and qu runs by its
; full path or from the Start-menu shortcut.
Section "Add qu to the PATH" SecPath
  DetailPrint "Adding $INSTDIR to the PATH"
  !insertmacro RUN_PATH_HELPER Add
  ${If} $0 == 0
    StrCpy $PathAdded 1
  ${ElseIf} $0 == 10
    DetailPrint "Already on the PATH; left unchanged"
  ${Else}
    DetailPrint "PATH update failed (helper exit code $0)"
    MessageBox MB_ICONEXCLAMATION "Qu was installed, but adding it to the PATH failed ($0). Add $INSTDIR to your PATH manually." /SD IDOK
    SetErrorLevel 3
  ${EndIf}
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "PathAdded" $PathAdded
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
SectionEnd

; On by default (/NOASSOC skips). Registers the .qu type ("Qu script", icon)
; and the verbs described at ASSOC_WRITE: double-click OPENS in an editor,
; "Run with Qu" is a right-click verb. Never executes on double-click.
Section "Register .qu files (Open in an editor; Run with Qu on right-click)" SecAssoc
  ${If} $AllUsers == 1
    !insertmacro ASSOC_WRITE HKLM
  ${Else}
    !insertmacro ASSOC_WRITE HKCU
  ${EndIf}
SectionEnd


!ifdef HAVE_JUPYTER
Section "Jupyter kernel (Qu in JupyterLab, Notebook, VS Code)" SecJupyter
  SetOutPath "$INSTDIR"
  File "${SRCDIR}\qu-jupyter.exe"
  File "qu-jupyter-start.cmd"
  ${If} $AllUsers == 1
    nsExec::ExecToLog '"$INSTDIR\qu-jupyter.exe" install --system'
  ${Else}
    nsExec::ExecToLog '"$INSTDIR\qu-jupyter.exe" install'
  ${EndIf}
  Pop $0
  ${If} $0 == 0
    WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "Jupyter" 1
  ${Else}
    DetailPrint "Registering the Jupyter kernel failed (exit code $0)"
    MessageBox MB_ICONEXCLAMATION "Qu was installed, but registering its Jupyter kernel failed ($0). Run:$\r$\n    qu-jupyter install" /SD IDOK
  ${EndIf}
SectionEnd
!endif

!ifdef HAVE_DOCS
Section /o "Offline documentation (about 25 MB)" SecDocs
  SetOutPath "$INSTDIR\docs"
  File /r "${SRCDIR}\docs\*.*"
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "Docs" 1
SectionEnd
!endif

!ifdef HAVE_EDITORS
; Per-user editor folders, whatever the install scope: an editor reads its
; plugins from the profile of whoever runs it.
SectionGroup "Editor plugins" SecPlugins
  ; Plugins the CLI installs are not removed on uninstall (the installer
  ; does not know where it put them); the fallback copies are.
  !macro VSCODE_FAMILY NAME DIR INIKEY
    SetShellVarContext current
    !insertmacro EDITOR_VIA_CLI ${NAME}
    ${If} $R7 != 0
      StrCpy $R5 "$PROFILE\${DIR}\extensions\qu-language-${VSCODE_EXT_VER}"
      SetOutPath $R5
      File /r "${SRCDIR}\editors\vscode-qu\*.*"
      WriteINIStr "$INSTDIR\${STATE_FILE}" "Plugins" "${INIKEY}" $R5
    ${EndIf}
    !insertmacro SET_SCOPE_CONTEXT
  !macroend
  Section "VS Code" SecVSCode
    !insertmacro VSCODE_FAMILY vscode ".vscode" "VSCode"
  SectionEnd
  Section "VSCodium" SecVSCodium
    !insertmacro VSCODE_FAMILY vscodium ".vscode-oss" "VSCodium"
  SectionEnd
  Section "Cursor" SecCursor
    !insertmacro VSCODE_FAMILY cursor ".cursor" "Cursor"
  SectionEnd
  Section "Windsurf" SecWindsurf
    !insertmacro VSCODE_FAMILY windsurf ".windsurf" "Windsurf"
  SectionEnd
  Section "Sublime Text" SecSublime
    SetShellVarContext current
    !insertmacro EDITOR_VIA_CLI sublime
    ${If} $R7 != 0
      ${If} ${FileExists} "$APPDATA\Sublime Text 3\Packages\*.*"
      ${AndIfNot} ${FileExists} "$APPDATA\Sublime Text\Packages\*.*"
        StrCpy $R5 "$APPDATA\Sublime Text 3\Packages\Qu"
      ${Else}
        StrCpy $R5 "$APPDATA\Sublime Text\Packages\Qu"
      ${EndIf}
      SetOutPath $R5
      File "${SRCDIR}\editors\sublime-qu\Qu.sublime-syntax"
      File "${SRCDIR}\editors\sublime-qu\Qu.sublime-build"
      WriteINIStr "$INSTDIR\${STATE_FILE}" "Plugins" "Sublime" $R5
    ${EndIf}
    !insertmacro SET_SCOPE_CONTEXT
  SectionEnd
  Section "Notepad++" SecNpp
    SetShellVarContext current
    !insertmacro EDITOR_VIA_CLI notepadpp
    ${If} $R7 != 0
      SetOutPath "$APPDATA\Notepad++\userDefineLangs"
      File "${SRCDIR}\editors\notepadpp-qu\Qu.udl.xml"
      WriteINIStr "$INSTDIR\${STATE_FILE}" "Plugins" "Notepadpp" "$APPDATA\Notepad++\userDefineLangs\Qu.udl.xml"
    ${EndIf}
    !insertmacro SET_SCOPE_CONTEXT
  SectionEnd
SectionGroupEnd
!endif

Section "Start menu shortcuts" SecStartMenu
  CreateDirectory "${SM_DIR}"
  ; qu.exe is a console program: the shortcut opens a console in the REPL
  CreateShortCut "${SM_DIR}\Qu CLI (REPL).lnk" "$INSTDIR\qu.exe" "repl" "$INSTDIR\qu.exe" 0 SW_SHOWNORMAL "" "Qu's interactive prompt"
  !ifdef HAVE_JUPYTER
    ${If} ${SectionIsSelected} ${SecJupyter}
      CreateShortCut "${SM_DIR}\Start Jupyter (Qu).lnk" "$INSTDIR\qu-jupyter-start.cmd" "" "$INSTDIR\qu.exe" 0 SW_SHOWNORMAL "" "JupyterLab with the Qu kernel"
    ${EndIf}
  !endif
  !ifdef HAVE_DOCS
    ${If} ${SectionIsSelected} ${SecDocs}
      CreateShortCut "${SM_DIR}\Qu Documentation.lnk" "$INSTDIR\docs\index.html"
    ${EndIf}
  !endif
  CreateShortCut "${SM_DIR}\Uninstall Qu.lnk" "$INSTDIR\uninstall.exe" "" "$INSTDIR\uninstall.exe" 0
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "StartMenu" 1
SectionEnd

Section /o "Desktop shortcuts" SecDesktop
  CreateShortCut "$DESKTOP\Qu CLI (REPL).lnk" "$INSTDIR\qu.exe" "repl" "$INSTDIR\qu.exe" 0
  !ifdef HAVE_JUPYTER
    ${If} ${SectionIsSelected} ${SecJupyter}
      CreateShortCut "$DESKTOP\Start Jupyter (Qu).lnk" "$INSTDIR\qu-jupyter-start.cmd" "" "$INSTDIR\qu.exe" 0
    ${EndIf}
  !endif
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "Desktop" 1
SectionEnd

; After the sections: their ${Sec...} ids exist only from here on.
Function QuComponentDefaults
  ; Component defaults: plugins only for editors that are here; the
  ; command-line switches override the page for scripted installs.
  !ifdef HAVE_EDITORS
    !insertmacro DETECT_EDITOR ${SecVSCode} "VS Code" "Code.exe" "$PROFILE\.vscode" "$APPDATA\Code"
    !insertmacro DETECT_EDITOR ${SecVSCodium} "VSCodium" "codium.exe" "$PROFILE\.vscode-oss" "$APPDATA\VSCodium"
    !insertmacro DETECT_EDITOR ${SecCursor} "Cursor" "cursor.exe" "$PROFILE\.cursor" "$APPDATA\Cursor"
    !insertmacro DETECT_EDITOR ${SecWindsurf} "Windsurf" "windsurf.exe" "$PROFILE\.windsurf" "$APPDATA\Windsurf"
    !insertmacro DETECT_EDITOR ${SecSublime} "Sublime Text" "sublime_text.exe" "$APPDATA\Sublime Text\Packages" "$APPDATA\Sublime Text 3\Packages"
    !insertmacro DETECT_EDITOR ${SecNpp} "Notepad++" "notepad++.exe" "$APPDATA\Notepad++" "$APPDATA\Notepad++"
  !endif
  ClearErrors
  ${GetParameters} $R0
  ${GetOptions} $R0 "/NOASSOC" $R1
  ${IfNot} ${Errors}
    !insertmacro UnselectSection ${SecAssoc}
  ${EndIf}
  ${GetParameters} $R0
  !ifdef HAVE_DOCS
    ClearErrors
    ${GetOptions} $R0 "/WITHDOCS" $R1
    ${IfNot} ${Errors}
      !insertmacro SelectSection ${SecDocs}
    ${EndIf}
  !endif
  !ifdef HAVE_JUPYTER
    ClearErrors
    ${GetOptions} $R0 "/NOJUPYTER" $R1
    ${IfNot} ${Errors}
      !insertmacro UnselectSection ${SecJupyter}
    ${EndIf}
  !endif
  !ifdef HAVE_EDITORS
    ClearErrors
    ${GetOptions} $R0 "/NOPLUGINS" $R1
    ${IfNot} ${Errors}
      !insertmacro UnselectSection ${SecVSCode}
      !insertmacro UnselectSection ${SecVSCodium}
      !insertmacro UnselectSection ${SecCursor}
      !insertmacro UnselectSection ${SecWindsurf}
      !insertmacro UnselectSection ${SecSublime}
      !insertmacro UnselectSection ${SecNpp}
    ${EndIf}
  !endif
  ClearErrors
  ${GetOptions} $R0 "/NOPATH" $R1
  ${IfNot} ${Errors}
    !insertmacro UnselectSection ${SecPath}
  ${EndIf}
  ClearErrors
  ${GetOptions} $R0 "/NOSHORTCUTS" $R1
  ${IfNot} ${Errors}
    !insertmacro UnselectSection ${SecStartMenu}
  ${EndIf}
  ClearErrors
  ${GetOptions} $R0 "/DESKTOP" $R1
  ${IfNot} ${Errors}
    !insertmacro SelectSection ${SecDesktop}
  ${EndIf}
FunctionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "qu.exe, its licence files and the uninstaller."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAssoc} "Gives .qu files the name $\"Qu script$\" and an icon. Double-clicking OPENS the file in a text editor (Qu Studio if you install it, else Notepad); it never runs it. Right-click, Run with Qu, runs it. An existing .qu default is not taken over."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecPath} "Lets any new terminal run qu by name. Only this one entry is added; the rest of PATH is left exactly as it is, and uninstalling removes it."
  !ifdef HAVE_JUPYTER
    !insertmacro MUI_DESCRIPTION_TEXT ${SecJupyter} "qu-jupyter.exe, registered as the $\"Qu$\" kernel for JupyterLab, Notebook and VS Code's Jupyter extension. Jupyter itself is installed separately (pip install jupyterlab)."
  !endif
  !ifdef HAVE_DOCS
    !insertmacro MUI_DESCRIPTION_TEXT ${SecDocs} "The full reference and guides as local HTML. help(name) then opens the local page."
  !endif
  !ifdef HAVE_EDITORS
    !insertmacro MUI_DESCRIPTION_TEXT ${SecPlugins} "Syntax highlighting and run commands for Qu files. Each is preselected only if that editor is installed."
  !endif
  !insertmacro MUI_DESCRIPTION_TEXT ${SecStartMenu} "Start menu: Qu CLI (REPL), Start Jupyter (Qu), Qu Documentation."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Qu CLI (REPL) and Start Jupyter (Qu) on the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ---- uninstaller --------------------------------------------------------

Function un.onInit
  ReadINIStr $AllUsers "$INSTDIR\${STATE_FILE}" "Install" "AllUsers"
  ${If} $AllUsers != 1
    StrCpy $AllUsers 0
  ${EndIf}
  ${If} $AllUsers == 1
    ; Run the elevated copy in place (_?=) so it does not re-copy itself.
    !insertmacro RELAUNCH_ELEVATED "_?=" $INSTDIR
  ${EndIf}
  !insertmacro SET_SCOPE_CONTEXT
FunctionEnd

Section "Uninstall"
  ReadINIStr $PathAdded "$INSTDIR\${STATE_FILE}" "Install" "PathAdded"
  ${If} $PathAdded == 1
    ${If} ${FileExists} "$INSTDIR\path-helper.ps1"
      DetailPrint "Removing $INSTDIR from the PATH"
      !insertmacro RUN_PATH_HELPER Remove
      ${If} $0 == 10
        DetailPrint "Not on the PATH any more; nothing to remove"
      ${ElseIf} $0 != 0
        DetailPrint "PATH update failed (helper exit code $0)"
        MessageBox MB_ICONEXCLAMATION "Removing $INSTDIR from the PATH failed ($0). Remove it manually." /SD IDOK
        SetErrorLevel 3
      ${EndIf}
      SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
    ${EndIf}
  ${Else}
    DetailPrint "This install did not add the PATH entry; PATH left unchanged"
  ${EndIf}

  ; Optional components, from what install.ini says was installed.
  ReadINIStr $R0 "$INSTDIR\${STATE_FILE}" "Install" "Jupyter"
  ${If} $R0 == 1
  ${AndIf} ${FileExists} "$INSTDIR\qu-jupyter.exe"
    ${If} $AllUsers == 1
      nsExec::ExecToLog '"$INSTDIR\qu-jupyter.exe" uninstall --system'
    ${Else}
      nsExec::ExecToLog '"$INSTDIR\qu-jupyter.exe" uninstall'
    ${EndIf}
    Pop $0
  ${EndIf}
  Delete "$INSTDIR\qu-jupyter.exe"
  Delete "$INSTDIR\qu-jupyter-start.cmd"
  ; docs\ is this installer's own folder, never one the user chose
  ReadINIStr $R0 "$INSTDIR\${STATE_FILE}" "Install" "Docs"
  ${If} $R0 == 1
    RMDir /r "$INSTDIR\docs"
  ${EndIf}
  ${If} $AllUsers == 1
    !insertmacro ASSOC_REMOVE HKLM
  ${Else}
    !insertmacro ASSOC_REMOVE HKCU
  ${EndIf}
  ReadINIStr $R0 "$INSTDIR\${STATE_FILE}" "Plugins" "VSCode"
  ${If} $R0 != ""
    RMDir /r $R0
  ${EndIf}
  ReadINIStr $R0 "$INSTDIR\${STATE_FILE}" "Plugins" "VSCodium"
  ${If} $R0 != ""
    RMDir /r $R0
  ${EndIf}
  ReadINIStr $R0 "$INSTDIR\${STATE_FILE}" "Plugins" "Cursor"
  ${If} $R0 != ""
    RMDir /r $R0
  ${EndIf}
  ReadINIStr $R0 "$INSTDIR\${STATE_FILE}" "Plugins" "Windsurf"
  ${If} $R0 != ""
    RMDir /r $R0
  ${EndIf}
  ReadINIStr $R0 "$INSTDIR\${STATE_FILE}" "Plugins" "Sublime"
  ${If} $R0 != ""
    Delete "$R0\Qu.sublime-syntax"
    Delete "$R0\Qu.sublime-build"
    RMDir $R0
  ${EndIf}
  ReadINIStr $R0 "$INSTDIR\${STATE_FILE}" "Plugins" "Notepadpp"
  ${If} $R0 != ""
    Delete $R0
  ${EndIf}
  Delete "${SM_DIR}\Qu CLI (REPL).lnk"
  Delete "${SM_DIR}\Start Jupyter (Qu).lnk"
  Delete "${SM_DIR}\Qu Documentation.lnk"
  Delete "${SM_DIR}\Uninstall Qu.lnk"
  RMDir "${SM_DIR}"
  Delete "$DESKTOP\Qu CLI (REPL).lnk"
  Delete "$DESKTOP\Start Jupyter (Qu).lnk"

  ; Only the files this installer put there -- never a recursive delete of
  ; a directory the user chose, which could hold their own files.
  Delete "$INSTDIR\qu.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\NOTICE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\path-helper.ps1"
  Delete "$INSTDIR\${STATE_FILE}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  ${If} $AllUsers == 1
    DeleteRegKey HKLM "${UNINST_KEY}"
  ${Else}
    DeleteRegKey HKCU "${UNINST_KEY}"
  ${EndIf}
SectionEnd
