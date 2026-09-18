; Tauri 1.x NSIS installer hooks for Qu Studio.
;
; Two things, both best-effort and silent (no extra dialogs -- if an
; editor isn't found, its step is simply skipped):
;   1. Add the app's install directory (where the bundled `qu` sidecar
;      binary lives) to the CURRENT USER's PATH, so `qu` works from an
;      ordinary terminal, not just from inside Qu Studio.
;   2. Detect VS Code / Sublime Text / Notepad++ by the presence of
;      their real per-user config folders (created the first time each
;      editor has actually run), and if found, copy this repo's
;      editors/<name>-qu/ integration into place -- the exact same
;      files and the exact same destination each editor's own README
;      documents as the manual "Option A" install.
;
; NOT live-tested against a real installer run from this environment --
; written against Tauri's documented NSIS_HOOK_* macro contract and
; standard, widely-used NSIS patterns (self-contained StrStr, no
; external plugins), but verify on a real Windows box before shipping.
; If something's wrong, both steps degrade safely: PATH just doesn't
; gain the entry, or an editor integration doesn't get copied -- Qu
; Studio itself still installs and runs either way.

!macro NSIS_HOOK_PREINSTALL
  ; Nothing needed before install.
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Call AddInstDirToUserPath
  Call InstallVSCodeExtension
  Call InstallSublimePackage
  Call InstallNotepadPlusPlusUDL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Call un.RemoveInstDirFromUserPath
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Deliberately NOT removing the editor integrations on uninstall --
  ; they're small, harmless standalone files the user may still want
  ; (syntax highlighting works even with the engine gone), and Sublime/
  ; Notepad++ have no "extension uninstall" concept to hook into
  ; cleanly anyway. Removing them silently on a Qu Studio uninstall
  ; would be a surprise, not a courtesy.
!macroend

; ---------------------------------------------------------------------
; PATH (current user, not machine-wide -- no admin elevation needed)
; ---------------------------------------------------------------------

Function AddInstDirToUserPath
  ReadRegStr $0 HKCU "Environment" "Path"

  ; Empty PATH: just set it.
  StrCmp $0 "" 0 CheckExisting
    WriteRegExpandStr HKCU "Environment" "Path" "$INSTDIR"
    Goto BroadcastChange

  CheckExisting:
    Push $0
    Push "$INSTDIR"
    Call StrStr
    Pop $1
    StrCmp $1 "" 0 AlreadyPresent
      ; Not found -- append with a separating semicolon.
      WriteRegExpandStr HKCU "Environment" "Path" "$0;$INSTDIR"
      Goto BroadcastChange

  AlreadyPresent:
    ; Re-running the installer over an existing install -- don't
    ; duplicate the entry.
    Goto Done

  BroadcastChange:
    ; Tell running processes (e.g. an already-open terminal won't pick
    ; this up until restarted regardless, but Explorer and anything
    ; that listens does) that the environment changed.
    SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000

  Done:
FunctionEnd

Function un.RemoveInstDirFromUserPath
  ReadRegStr $0 HKCU "Environment" "Path"
  StrCmp $0 "" Done

  Push $0
  Push "$INSTDIR"
  Call un.StrStr
  Pop $1
  StrCmp $1 "" Done
    ; $1 is the tail of $0 starting at the match. Rebuild PATH with
    ; "$INSTDIR" and one adjacent separator removed, wherever it sits.
    StrLen $2 $1
    StrLen $3 $0
    IntOp $3 $3 - $2         ; length of the part before the match
    StrCpy $4 $0 $3          ; everything before "$INSTDIR"
    StrLen $5 "$INSTDIR"
    IntOp $5 $2 - $5
    StrCpy $6 $1 $5 -$5      ; wrong direction guard -- recompute cleanly below
    StrCpy $6 $0 "" $3       ; from the match onward
    StrCpy $6 $6 "" $5       ; drop "$INSTDIR" itself, keep what follows
    ; Trim one leading/trailing semicolon so we don't leave ";;" or a
    ; dangling separator at either end.
    StrCpy $7 "$4$6"
    Push $7
    Call un.TrimSemicolons
    Pop $7
    WriteRegExpandStr HKCU "Environment" "Path" $7
    SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000

  Done:
FunctionEnd

Function un.TrimSemicolons
  Exch $0
  StrCpy $1 $0 1
  StrCmp $1 ";" 0 +2
    StrCpy $0 $0 "" 1
  StrLen $2 $0
  IntOp $2 $2 - 1
  StrCpy $1 $0 1 $2
  StrCmp $1 ";" 0 +2
    StrCpy $0 $0 $2
  Exch $0
FunctionEnd

; Self-contained substring search (no external plugin). Push haystack,
; then needle; pops the tail of haystack starting at the first match,
; or "" if not found. Standard, widely-used NSIS idiom.
Function StrStr
  Exch $R1 ; needle
  Exch
  Exch $R2 ; haystack
  Push $R3
  Push $R4
  Push $R5
  StrLen $R3 $R1
  StrCpy $R4 0
  StrStrLoop:
    StrCpy $R5 $R2 $R3 $R4
    StrCmp $R5 $R1 StrStrFound
    StrCmp $R5 "" StrStrNotFound
    IntOp $R4 $R4 + 1
    Goto StrStrLoop
  StrStrFound:
    StrCpy $R1 $R2 "" $R4
    Goto StrStrDone
  StrStrNotFound:
    StrCpy $R1 ""
  StrStrDone:
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Exch $R1
FunctionEnd

Function un.StrStr
  Exch $R1
  Exch
  Exch $R2
  Push $R3
  Push $R4
  Push $R5
  StrLen $R3 $R1
  StrCpy $R4 0
  un.StrStrLoop:
    StrCpy $R5 $R2 $R3 $R4
    StrCmp $R5 $R1 un.StrStrFound
    StrCmp $R5 "" un.StrStrNotFound
    IntOp $R4 $R4 + 1
    Goto un.StrStrLoop
  un.StrStrFound:
    StrCpy $R1 $R2 "" $R4
    Goto un.StrStrDone
  un.StrStrNotFound:
    StrCpy $R1 ""
  un.StrStrDone:
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Exch $R1
FunctionEnd

; ---------------------------------------------------------------------
; Editor detection + install
; ---------------------------------------------------------------------

; VS Code: detected by its real per-user extensions folder, created the
; first time VS Code itself has actually run -- not a registry probe,
; which is a version-and-install-type-specific moving target (user vs.
; system installer, VS Code vs. VS Code Insiders each have their own
; keys). This is the exact same signal, and exact same destination, the
; extension's own README documents as its manual "Option A".
Function InstallVSCodeExtension
  IfFileExists "$PROFILE\.vscode\*.*" 0 Done
    CreateDirectory "$PROFILE\.vscode\extensions\qu-language-0.1.0"
    CopyFiles /SILENT "$INSTDIR\resources\editors\vscode-qu\*.*" "$PROFILE\.vscode\extensions\qu-language-0.1.0"
  Done:
FunctionEnd

; Sublime Text: both ST3's and ST4's Packages-folder naming, since
; either may exist depending which version the user has.
Function InstallSublimePackage
  IfFileExists "$APPDATA\Sublime Text\Packages\*.*" 0 TrySublime3
    CreateDirectory "$APPDATA\Sublime Text\Packages\Qu"
    CopyFiles /SILENT "$INSTDIR\resources\editors\sublime-qu\Qu.sublime-syntax" "$APPDATA\Sublime Text\Packages\Qu"
    CopyFiles /SILENT "$INSTDIR\resources\editors\sublime-qu\Qu.sublime-build" "$APPDATA\Sublime Text\Packages\Qu"
    Goto Done
  TrySublime3:
    IfFileExists "$APPDATA\Sublime Text 3\Packages\*.*" 0 Done
      CreateDirectory "$APPDATA\Sublime Text 3\Packages\Qu"
      CopyFiles /SILENT "$INSTDIR\resources\editors\sublime-qu\Qu.sublime-syntax" "$APPDATA\Sublime Text 3\Packages\Qu"
      CopyFiles /SILENT "$INSTDIR\resources\editors\sublime-qu\Qu.sublime-build" "$APPDATA\Sublime Text 3\Packages\Qu"
  Done:
FunctionEnd

; Notepad++: dropping a .xml directly into userDefineLangs\ is loaded
; automatically at next startup -- the GUI "Import..." step documented
; in the editor's own README does exactly this file copy under the
; hood, so this isn't a shortcut that skips real behaviour.
Function InstallNotepadPlusPlusUDL
  IfFileExists "$APPDATA\Notepad++\*.*" 0 Done
    CreateDirectory "$APPDATA\Notepad++\userDefineLangs"
    CopyFiles /SILENT "$INSTDIR\resources\editors\notepadpp-qu\Qu.udl.xml" "$APPDATA\Notepad++\userDefineLangs"
  Done:
FunctionEnd
