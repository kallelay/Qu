; Installer for the `qu` command-line binary on Windows.
;
; Build (CI does this in .github/workflows/release.yml):
;   makensis /DVERSION=0.4.4 /DARCH=x86_64 /DSRCDIR=<dir with qu.exe> ^
;            [/DOUTDIR=<output dir>] installer\windows\qu.nsi
; Produces qu-<VERSION>-windows-<ARCH>-setup.exe. ARCH is x86_64 or arm64.
;
; Runtime switches:
;   /S           silent
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

Var AllUsers     ; 1 = machine-wide
Var PathAdded    ; 1 = this install owns a PATH entry (persisted in install.ini)
Var PsExe

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_TEXT "The qu command is on your PATH. Open a new terminal and run:$\r$\n$\r$\n    qu --version"
!insertmacro MUI_PAGE_LICENSE "${SRCDIR}\LICENSE"
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
FunctionEnd

Section "Qu" SecMain
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
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "AllUsers" $AllUsers
  WriteINIStr "$INSTDIR\${STATE_FILE}" "Install" "Version" "${VERSION}"
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000

  ${GetSize} "$INSTDIR" "/S=0K" $R2 $R3 $R4
  ${If} $AllUsers == 1
    StrCpy $R1 " /ALLUSERS"
    !insertmacro WRITE_ARP HKLM
  ${Else}
    StrCpy $R1 ""
    !insertmacro WRITE_ARP HKCU
  ${EndIf}
SectionEnd


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
