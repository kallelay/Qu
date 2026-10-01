; Qu Studio NSIS installer template (Tauri 1.x, `tauri.bundle.windows.nsis.template`).
;
; This is Tauri's STOCK template from tauri-cli v1.6.3 / tauri-bundler 1.7.1
; (tauri-apps/tauri tag tauri-cli-v1.6.3, tooling/bundler/src/bundle/windows/
; templates/installer.nsi), kept verbatim except for lines marked "QU:".
; Everything Qu adds lives in the "QU ADDITIONS" block at the end of the file;
; the stock body only gains two call sites (end of `Section Install`, start
; of `Section Uninstall`). If @tauri-apps/cli is bumped, re-diff this file
; against the new stock template before building.
;
; What Qu adds:
;   1. $INSTDIR (where the bundled `qu.exe` sidecar is installed) is appended
;      to the CURRENT USER's PATH (HKCU\Environment\Path); the uninstaller
;      removes exactly that entry again.
;   2. A Components page (QU: stock sections renamed "-..." so they stay
;      hidden and always run): Jupyter kernel [on], offline documentation
;      [off] and editor plugins (VS Code, Sublime Text, Notepad++; each on
;      only if that editor's config folder exists), plus Start-menu and
;      desktop shortcuts next to Qu Studio's own: "Qu CLI (REPL)",
;      "Start Jupyter (Qu)", "Qu Documentation". The Jupyter and docs
;      payloads come from $%QU_STUDIO_EXTRAS% at build time (release.yml
;      stages qu-jupyter.exe, qu-jupyter-start.cmd and docs\ there); a
;      build without it simply has no such components.
;      Silent switches: /WITHDOCS /NOJUPYTER /NOPLUGINS /NOPATH.
;
; Tauri 2 migration note: Tauri 2 has `bundle.windows.nsis.installerHooks`
; (NSIS_HOOK_POSTINSTALL / NSIS_HOOK_PREUNINSTALL). The migration should
; drop this whole file and the `template` key, move the QU ADDITIONS block
; into a hooks .nsh, and call QuPostInstall / un.QuPreUninstall from those
; two hook macros. The resource path in QU_EDITORS_DIR must be re-checked
; then too (Tauri 2 maps `../` in resource paths the same way, `_up_`).

Unicode true
; Set the compression algorithm. Default is LZMA.
!if "{{compression}}" == ""
  SetCompressor /SOLID lzma
!else
  SetCompressor /SOLID "{{compression}}"
!endif

!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
!include "StrFunc.nsh"
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "Sections.nsh" ; QU: SelectSection/SectionIsSelected for the components
${StrCase}
${StrLoc}

!define MANUFACTURER "{{manufacturer}}"
!define PRODUCTNAME "{{product_name}}"
!define VERSION "{{version}}"
!define VERSIONWITHBUILD "{{version_with_build}}"
!define INSTALLMODE "{{install_mode}}"
!define LICENSE "{{license}}"
!define INSTALLERICON "{{installer_icon}}"
!define SIDEBARIMAGE "{{sidebar_image}}"
!define HEADERIMAGE "{{header_image}}"
!define MAINBINARYNAME "{{main_binary_name}}"
!define MAINBINARYSRCPATH "{{main_binary_path}}"
!define BUNDLEID "{{bundle_id}}"
!define COPYRIGHT "{{copyright}}"
!define OUTFILE "{{out_file}}"
!define ARCH "{{arch}}"
!define PLUGINSPATH "{{additional_plugins_path}}"
!define ALLOWDOWNGRADES "{{allow_downgrades}}"
!define DISPLAYLANGUAGESELECTOR "{{display_language_selector}}"
!define INSTALLWEBVIEW2MODE "{{install_webview2_mode}}"
!define WEBVIEW2INSTALLERARGS "{{webview2_installer_args}}"
!define WEBVIEW2BOOTSTRAPPERPATH "{{webview2_bootstrapper_path}}"
!define WEBVIEW2INSTALLERPATH "{{webview2_installer_path}}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define MANUPRODUCTKEY "Software\${MANUFACTURER}\${PRODUCTNAME}"
!define UNINSTALLERSIGNCOMMAND "{{uninstaller_sign_cmd}}"
!define ESTIMATEDSIZE "{{estimated_size}}"

Name "${PRODUCTNAME}"
BrandingText "${COPYRIGHT}"
OutFile "${OUTFILE}"

VIProductVersion "${VERSIONWITHBUILD}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "LegalCopyright" "${COPYRIGHT}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

; Plugins path, currently exists for linux only
!if "${PLUGINSPATH}" != ""
    !addplugindir "${PLUGINSPATH}"
!endif

!if "${UNINSTALLERSIGNCOMMAND}" != ""
  !uninstfinalize '${UNINSTALLERSIGNCOMMAND}'
!endif

; Handle install mode, `perUser`, `perMachine` or `both`
!if "${INSTALLMODE}" == "perMachine"
  RequestExecutionLevel highest
!endif

!if "${INSTALLMODE}" == "currentUser"
  RequestExecutionLevel user
!endif

!if "${INSTALLMODE}" == "both"
  !define MULTIUSER_MUI
  !define MULTIUSER_INSTALLMODE_INSTDIR "${PRODUCTNAME}"
  !define MULTIUSER_INSTALLMODE_COMMANDLINE
  !if "${ARCH}" == "x64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !else if "${ARCH}" == "arm64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !endif
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_KEY "${UNINSTKEY}"
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_VALUENAME "CurrentUser"
  !define MULTIUSER_INSTALLMODEPAGE_SHOWUSERNAME
  !define MULTIUSER_INSTALLMODE_FUNCTION RestorePreviousInstallLocation
  !define MULTIUSER_EXECUTIONLEVEL Highest
  !include MultiUser.nsh
!endif

; installer icon
!if "${INSTALLERICON}" != ""
  !define MUI_ICON "${INSTALLERICON}"
!endif

; installer sidebar image
!if "${SIDEBARIMAGE}" != ""
  !define MUI_WELCOMEFINISHPAGE_BITMAP "${SIDEBARIMAGE}"
!endif

; installer header image
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
  !define MUI_HEADERIMAGE_BITMAP  "${HEADERIMAGE}"
!endif

; Define registry key to store installer language
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "${MANUPRODUCTKEY}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Installer Language"

; Installer pages, must be ordered as they appear
; 1. Welcome Page
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_WELCOME

; 2. License Page (if defined)
!if "${LICENSE}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !insertmacro MUI_PAGE_LICENSE "${LICENSE}"
!endif

; 3. Install mode (if it is set to `both`)
!if "${INSTALLMODE}" == "both"
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !insertmacro MULTIUSER_PAGE_INSTALLMODE
!endif


; 4. Custom page to ask user if he wants to reinstall/uninstall
;    only if a previous installtion was detected
Var ReinstallPageCheck
Page custom PageReinstall PageLeaveReinstall
Function PageReinstall
  ; Uninstall previous WiX installation if exists.
  ;
  ; A WiX installer stores the isntallation info in registry
  ; using a UUID and so we have to loop through all keys under
  ; `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
  ; and check if `DisplayName` and `Publisher` keys match ${PRODUCTNAME} and ${MANUFACTURER}
  ;
  ; This has a potentional issue that there maybe another installation that matches
  ; our ${PRODUCTNAME} and ${MANUFACTURER} but wasn't installed by our WiX installer,
  ; however, this should be fine since the user will have to confirm the uninstallation
  ; and they can chose to abort it if doesn't make sense.
  StrCpy $0 0
  wix_loop:
    EnumRegKey $1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $0
    StrCmp $1 "" wix_done ; Exit loop if there is no more keys to loop on
    IntOp $0 $0 + 1
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "Publisher"
    StrCmp "$R0$R1" "${PRODUCTNAME}${MANUFACTURER}" 0 wix_loop
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "UninstallString"
    ${StrCase} $R1 $R0 "L"
    ${StrLoc} $R0 $R1 "msiexec" ">"
    StrCmp $R0 0 0 wix_done
    StrCpy $R7 "wix"
    StrCpy $R6 "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1"
    Goto compare_version
  wix_done:

  ; Check if there is an existing installation, if not, abort the reinstall page
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" ""
  ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
  ${IfThen} "$R0$R1" == "" ${|} Abort ${|}

  ; Compare this installar version with the existing installation
  ; and modify the messages presented to the user accordingly
  compare_version:
  StrCpy $R4 "$(older)"
  ${If} $R7 == "wix"
    ReadRegStr $R0 HKLM "$R6" "DisplayVersion"
  ${Else}
    ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  ${EndIf}
  ${IfThen} $R0 == "" ${|} StrCpy $R4 "$(unknown)" ${|}

  nsis_tauri_utils::SemverCompare "${VERSION}" $R0
  Pop $R0
  ; Reinstalling the same version
  ${If} $R0 == 0
    StrCpy $R1 "$(alreadyInstalledLong)"
    StrCpy $R2 "$(addOrReinstall)"
    StrCpy $R3 "$(uninstallApp)"
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$(chooseMaintenanceOption)"
    StrCpy $R5 "2"
  ; Upgrading
  ${ElseIf} $R0 == 1
    StrCpy $R1 "$(olderOrUnknownVersionInstalled)"
    StrCpy $R2 "$(uninstallBeforeInstalling)"
    StrCpy $R3 "$(dontUninstall)"
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$(choowHowToInstall)"
    StrCpy $R5 "1"
  ; Downgrading
  ${ElseIf} $R0 == -1
    StrCpy $R1 "$(newerVersionInstalled)"
    StrCpy $R2 "$(uninstallBeforeInstalling)"
    !if "${ALLOWDOWNGRADES}" == "true"
      StrCpy $R3 "$(dontUninstall)"
    !else
      StrCpy $R3 "$(dontUninstallDowngrade)"
    !endif
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$(choowHowToInstall)"
    StrCpy $R5 "1"
  ${Else}
    Abort
  ${EndIf}

  Call SkipIfPassive

  nsDialogs::Create 1018
  Pop $R4
  ${IfThen} $(^RTL) == 1 ${|} nsDialogs::SetRTL $(^RTL) ${|}

  ${NSD_CreateLabel} 0 0 100% 24u $R1
  Pop $R1

  ${NSD_CreateRadioButton} 30u 50u -30u 8u $R2
  Pop $R2
  ${NSD_OnClick} $R2 PageReinstallUpdateSelection

  ${NSD_CreateRadioButton} 30u 70u -30u 8u $R3
  Pop $R3
  ; disable this radio button if downgrading and downgrades are disabled
  !if "${ALLOWDOWNGRADES}" == "false"
    ${IfThen} $R0 == -1 ${|} EnableWindow $R3 0 ${|}
  !endif
  ${NSD_OnClick} $R3 PageReinstallUpdateSelection

  ; Check the first radio button if this the first time
  ; we enter this page or if the second button wasn't
  ; selected the last time we were on this page
  ${If} $ReinstallPageCheck != 2
    SendMessage $R2 ${BM_SETCHECK} ${BST_CHECKED} 0
  ${Else}
    SendMessage $R3 ${BM_SETCHECK} ${BST_CHECKED} 0
  ${EndIf}

  ${NSD_SetFocus} $R2
  nsDialogs::Show
FunctionEnd
Function PageReinstallUpdateSelection
  ${NSD_GetState} $R2 $R1
  ${If} $R1 == ${BST_CHECKED}
    StrCpy $ReinstallPageCheck 1
  ${Else}
    StrCpy $ReinstallPageCheck 2
  ${EndIf}
FunctionEnd
Function PageLeaveReinstall
  ${NSD_GetState} $R2 $R1

  ; $R5 holds whether we are reinstalling the same version or not
  ; $R5 == "1" -> different versions
  ; $R5 == "2" -> same version
  ;
  ; $R1 holds the radio buttons state. its meaning is dependant on the context
  StrCmp $R5 "1" 0 +2 ; Existing install is not the same version?
    StrCmp $R1 "1" reinst_uninstall reinst_done ; $R1 == "1", then user chose to uninstall existing version, otherwise skip uninstalling
  StrCmp $R1 "1" reinst_done ; Same version? skip uninstalling

  reinst_uninstall:
    HideWindow
    ClearErrors

    ${If} $R7 == "wix"
      ReadRegStr $R1 HKLM "$R6" "UninstallString"
      ExecWait '$R1' $0
    ${Else}
      ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
      ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
      ExecWait '$R1 /P _?=$4' $0
    ${EndIf}

    BringToFront

    ${IfThen} ${Errors} ${|} StrCpy $0 2 ${|} ; ExecWait failed, set fake exit code

    ${If} $0 <> 0
    ${OrIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
      ${If} $0 = 1 ; User aborted uninstaller?
        StrCmp $R5 "2" 0 +2 ; Is the existing install the same version?
          Quit ; ...yes, already installed, we are done
        Abort
      ${EndIf}
      MessageBox MB_ICONEXCLAMATION "$(unableToUninstall)"
      Abort
    ${Else}
      StrCpy $0 $R1 1
      ${IfThen} $0 == '"' ${|} StrCpy $R1 $R1 -1 1 ${|} ; Strip quotes from UninstallString
      Delete $R1
      RMDir $INSTDIR
    ${EndIf}
  reinst_done:
FunctionEnd

; QU: optional components (see QU ADDITIONS)
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_COMPONENTS

; 5. Choose install directoy page
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_DIRECTORY

; 6. Start menu shortcut page
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
Var AppStartMenuFolder
!insertmacro MUI_PAGE_STARTMENU Application $AppStartMenuFolder

; 7. Installation page
!insertmacro MUI_PAGE_INSTFILES

; 8. Finish page
;
; Don't auto jump to finish page after installation page,
; because the installation page has useful info that can be used debug any issues with the installer.
!define MUI_FINISHPAGE_NOAUTOCLOSE
; Use show readme button in the finish page as a button create a desktop shortcut
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "$(createDesktop)"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateDesktopShortcut
; Show run app after installation.
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION RunMainBinary
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_FINISH

Function RunMainBinary
  nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" ""
FunctionEnd

; Uninstaller Pages
; 1. Confirm uninstall page
Var DeleteAppDataCheckbox
Var DeleteAppDataCheckboxState
!define /ifndef WS_EX_LAYOUTRTL         0x00400000
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.ConfirmShow
Function un.ConfirmShow
    FindWindow $1 "#32770" "" $HWNDPARENT ; Find inner dialog
    ${If} $(^RTL) == 1
      System::Call 'USER32::CreateWindowEx(i${__NSD_CheckBox_EXSTYLE}|${WS_EX_LAYOUTRTL},t"${__NSD_CheckBox_CLASS}",t "$(deleteAppData)",i${__NSD_CheckBox_STYLE},i 50,i 100,i 400, i 25,i$1,i0,i0,i0)i.s'
    ${Else}
      System::Call 'USER32::CreateWindowEx(i${__NSD_CheckBox_EXSTYLE},t"${__NSD_CheckBox_CLASS}",t "$(deleteAppData)",i${__NSD_CheckBox_STYLE},i 0,i 100,i 400, i 25,i$1,i0,i0,i0)i.s'
    ${EndIf}
    Pop $DeleteAppDataCheckbox
    SendMessage $HWNDPARENT ${WM_GETFONT} 0 0 $1
    SendMessage $DeleteAppDataCheckbox ${WM_SETFONT} $1 1
FunctionEnd
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE un.ConfirmLeave
Function un.ConfirmLeave
    SendMessage $DeleteAppDataCheckbox ${BM_GETCHECK} 0 0 $DeleteAppDataCheckboxState
FunctionEnd
!insertmacro MUI_UNPAGE_CONFIRM

; 2. Uninstalling Page
!insertmacro MUI_UNPAGE_INSTFILES

;Languages
{{#each languages}}
!insertmacro MUI_LANGUAGE "{{this}}"
{{/each}}
!insertmacro MUI_RESERVEFILE_LANGDLL
{{#each language_files}}
  !include "{{this}}"
{{/each}}

!macro SetContext
  !if "${INSTALLMODE}" == "currentUser"
    SetShellVarContext current
  !else if "${INSTALLMODE}" == "perMachine"
    SetShellVarContext all
  !endif

  ${If} ${RunningX64}
    !if "${ARCH}" == "x64"
      SetRegView 64
    !else if "${ARCH}" == "arm64"
      SetRegView 64
    !else
      SetRegView 32
    !endif
  ${EndIf}
!macroend

Var PassiveMode
Function .onInit
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  IfErrors +2 0
    StrCpy $PassiveMode 1

  !if "${DISPLAYLANGUAGESELECTOR}" == "true"
    !insertmacro MUI_LANGDLL_DISPLAY
  !endif

  !insertmacro SetContext

  ${If} $INSTDIR == ""
    ; Set default install location
    !if "${INSTALLMODE}" == "perMachine"
      ${If} ${RunningX64}
        !if "${ARCH}" == "x64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else if "${ARCH}" == "arm64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else
          StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
        !endif
      ${Else}
        StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
      ${EndIf}
    !else if "${INSTALLMODE}" == "currentUser"
      StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"
    !endif

    Call RestorePreviousInstallLocation
  ${EndIf}


  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_INIT
  !endif

  ; QU: component defaults and silent switches (QU ADDITIONS)
  Call QuComponentDefaults
FunctionEnd


Section -EarlyChecks ; QU: '-' hides it on the Components page
  ; Abort silent installer if downgrades is disabled
  !if "${ALLOWDOWNGRADES}" == "false"
  IfSilent 0 silent_downgrades_done
    ; If downgrading
    ${If} $R0 == -1
      System::Call 'kernel32::AttachConsole(i -1)i.r0'
      ${If} $0 != 0
        System::Call 'kernel32::GetStdHandle(i -11)i.r0'
        System::call 'kernel32::SetConsoleTextAttribute(i r0, i 0x0004)' ; set red color
        FileWrite $0 "$(silentDowngrades)"
      ${EndIf}
      Abort
    ${EndIf}
  silent_downgrades_done:
  !endif

SectionEnd

Section -WebView2 ; QU: hidden
  ; Check if Webview2 is already installed and skip this section
  ${If} ${RunningX64}
    ReadRegStr $4 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${Else}
    ReadRegStr $4 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${EndIf}
  ReadRegStr $5 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"

  StrCmp $4 "" 0 webview2_done
  StrCmp $5 "" 0 webview2_done

  ; Webview2 install modes
  !if "${INSTALLWEBVIEW2MODE}" == "downloadBootstrapper"
    Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    DetailPrint "$(webview2Downloading)"
    NSISdl::download "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    Pop $0
    ${If} $0 == "success"
      DetailPrint "$(webview2DownloadSuccess)"
    ${Else}
      DetailPrint "$(webview2DownloadError)"
      Abort "$(webview2AbortError)"
    ${EndIf}
    StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    Goto install_webview2
  !endif

  !if "${INSTALLWEBVIEW2MODE}" == "embedBootstrapper"
    Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    File "/oname=$TEMP\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"
    DetailPrint "$(installingWebview2)"
    StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    Goto install_webview2
  !endif

  !if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"
    Delete "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
    File "/oname=$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"
    DetailPrint "$(installingWebview2)"
    StrCpy $6 "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
    Goto install_webview2
  !endif

  Goto webview2_done

  install_webview2:
    DetailPrint "$(installingWebview2)"
    ; $6 holds the path to the webview2 installer
    ExecWait "$6 ${WEBVIEW2INSTALLERARGS} /install" $1
    ${If} $1 == 0
      DetailPrint "$(webview2InstallSuccess)"
    ${Else}
      DetailPrint "$(webview2InstallError)"
      Abort "$(webview2AbortError)"
    ${EndIf}
  webview2_done:
SectionEnd

!macro CheckIfAppIsRunning
  !if "${INSTALLMODE}" == "currentUser"
    nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
  !else
    nsis_tauri_utils::FindProcess "${MAINBINARYNAME}.exe"
  !endif
  Pop $R0
  ${If} $R0 = 0
      IfSilent kill 0
      ${IfThen} $PassiveMode != 1 ${|} MessageBox MB_OKCANCEL "$(appRunningOkKill)" IDOK kill IDCANCEL cancel ${|}
      kill:
        !if "${INSTALLMODE}" == "currentUser"
          nsis_tauri_utils::KillProcessCurrentUser "${MAINBINARYNAME}.exe"
        !else
          nsis_tauri_utils::KillProcess "${MAINBINARYNAME}.exe"
        !endif
        Pop $R0
        Sleep 500
        ${If} $R0 = 0
          Goto app_check_done
        ${Else}
          IfSilent silent ui
          silent:
            System::Call 'kernel32::AttachConsole(i -1)i.r0'
            ${If} $0 != 0
              System::Call 'kernel32::GetStdHandle(i -11)i.r0'
              System::call 'kernel32::SetConsoleTextAttribute(i r0, i 0x0004)' ; set red color
              FileWrite $0 "$(appRunning)$\n"
            ${EndIf}
            Abort
          ui:
            Abort "$(failedToKillApp)"
        ${EndIf}
      cancel:
        Abort "$(appRunning)"
  ${EndIf}
  app_check_done:
!macroend

Section -Install ; QU: hidden, always installed
  SetOutPath $INSTDIR

  !insertmacro CheckIfAppIsRunning

  ; Copy main executable
  File "${MAINBINARYSRCPATH}"

  ; Copy resources
  {{#each resources_dirs}}
    CreateDirectory "$INSTDIR\\{{this}}"
  {{/each}}
  {{#each resources}}
    File /a "/oname={{this.[1]}}" "{{unescape-dollar-sign @key}}"
  {{/each}}

  ; Copy external binaries
  {{#each binaries}}
    File /a "/oname={{this}}" "{{unescape-dollar-sign @key}}"
  {{/each}}

  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Save $INSTDIR in registry for future installations
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR

  !if "${INSTALLMODE}" == "both"
    ; Save install mode to be selected by default for the next installation such as updating
    ; or when uninstalling
    WriteRegStr SHCTX "${UNINSTKEY}" $MultiUser.InstallMode 1
  !endif

  ; Save current MAINBINARYNAME for future updates from v2 updater
  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}.exe"

  ; Registry information for add/remove programs
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoModify" "1"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" "1"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "EstimatedSize" "${ESTIMATEDSIZE}"

  ; Create start menu shortcut (GUI)
  !insertmacro MUI_STARTMENU_WRITE_BEGIN Application
    Call CreateStartMenuShortcut
  !insertmacro MUI_STARTMENU_WRITE_END

  ; Create shortcuts for silent and passive installers, which
  ; can be disabled by passing `/NS` flag
  ; GUI installer has buttons for users to control creating them
  IfSilent check_ns_flag 0
  ${IfThen} $PassiveMode == 1 ${|} Goto check_ns_flag ${|}
  Goto shortcuts_done
  check_ns_flag:
    ${GetOptions} $CMDLINE "/NS" $R0
    IfErrors 0 shortcuts_done
      Call CreateDesktopShortcut
      Call CreateStartMenuShortcut
  shortcuts_done:

  ; QU: PATH entry + editor integrations (see QU ADDITIONS at end of file)
  Call QuPostInstall

  ; Auto close this page for passive mode
  ${IfThen} $PassiveMode == 1 ${|} SetAutoClose true ${|}
SectionEnd

Function .onInstSuccess
  ; Check for `/R` flag only in silent and passive installers because
  ; GUI installer has a toggle for the user to (re)start the app
  IfSilent check_r_flag 0
  ${IfThen} $PassiveMode == 1 ${|} Goto check_r_flag ${|}
  Goto run_done
  check_r_flag:
    ${GetOptions} $CMDLINE "/R" $R0
    IfErrors run_done 0
      ${GetOptions} $CMDLINE "/ARGS" $R0
      nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "$R0"
  run_done:
FunctionEnd

Function un.onInit
  !insertmacro SetContext

  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_UNINIT
  !endif

  !insertmacro MUI_UNGETLANGUAGE
FunctionEnd

!macro DeleteAppUserModelId
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_DestinationList} ${IID_ICustomDestinationList} r1 ""
  ${If} $1 P<> 0
    ${ICustomDestinationList::DeleteList} $1 '("${BUNDLEID}")'
    ${IUnknown::Release} $1 ""
  ${EndIf}
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ApplicationDestinations} ${IID_IApplicationDestinations} r1 ""
  ${If} $1 P<> 0
    ${IApplicationDestinations::SetAppID} $1 '("${BUNDLEID}")i.r0'
    ${If} $0 >= 0
      ${IApplicationDestinations::RemoveAllDestinations} $1 ''
    ${EndIf}
    ${IUnknown::Release} $1 ""
  ${EndIf}
!macroend

; From https://stackoverflow.com/a/42816728/16993372
!macro UnpinShortcut shortcut
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_StartMenuPin} ${IID_IStartMenuPinnedList} r0 ""
  ${If} $0 P<> 0
      System::Call 'SHELL32::SHCreateItemFromParsingName(ws, p0, g "${IID_IShellItem}", *p0r1)' "${shortcut}"
      ${If} $1 P<> 0
          ${IStartMenuPinnedList::RemoveFromList} $0 '(r1)'
          ${IUnknown::Release} $1 ""
      ${EndIf}
      ${IUnknown::Release} $0 ""
  ${EndIf}
!macroend

Section Uninstall
  !insertmacro CheckIfAppIsRunning

  ; QU: remove our own PATH entry (see QU ADDITIONS at end of file)
  Call un.QuPreUninstall

  ; Delete the app directory and its content from disk
  ; Copy main executable
  Delete "$INSTDIR\${MAINBINARYNAME}.exe"

  ; Delete resources
  {{#each resources}}
    Delete "$INSTDIR\\{{this.[1]}}"
  {{/each}}

  ; Delete external binaries
  {{#each binaries}}
    Delete "$INSTDIR\\{{this}}"
  {{/each}}

  ; Delete uninstaller
  Delete "$INSTDIR\uninstall.exe"

  {{#each resources_ancestors}}
  RMDir /REBOOTOK "$INSTDIR\\{{this}}"
  {{/each}}
  RMDir "$INSTDIR"

  !insertmacro DeleteAppUserModelId
  !insertmacro UnpinShortcut "$SMPROGRAMS\$AppStartMenuFolder\${MAINBINARYNAME}.lnk"
  !insertmacro UnpinShortcut "$DESKTOP\${MAINBINARYNAME}.lnk"

  ; Remove start menu shortcut
  !insertmacro MUI_STARTMENU_GETFOLDER Application $AppStartMenuFolder
  Delete "$SMPROGRAMS\$AppStartMenuFolder\${MAINBINARYNAME}.lnk"
  RMDir "$SMPROGRAMS\$AppStartMenuFolder"

  ; Remove desktop shortcuts
  Delete "$DESKTOP\${MAINBINARYNAME}.lnk"

  ; Remove registry information for add/remove programs
  !if "${INSTALLMODE}" == "both"
    DeleteRegKey SHCTX "${UNINSTKEY}"
  !else if "${INSTALLMODE}" == "perMachine"
    DeleteRegKey HKLM "${UNINSTKEY}"
  !else
    DeleteRegKey HKCU "${UNINSTKEY}"
  !endif

  DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"

  ; Delete app data
  ${If} $DeleteAppDataCheckboxState == 1
    SetShellVarContext current
    RmDir /r "$APPDATA\${BUNDLEID}"
    RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}

  ${GetOptions} $CMDLINE "/P" $R0
  IfErrors +2 0
    SetAutoClose true
SectionEnd

Function RestorePreviousInstallLocation
  ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
  StrCmp $4 "" +2 0
    StrCpy $INSTDIR $4
FunctionEnd

Function SkipIfPassive
  ${IfThen} $PassiveMode == 1  ${|} Abort ${|}
FunctionEnd

!macro SetLnkAppUserModelId shortcut
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("${shortcut}", ${STGM_READWRITE})'
      ${IUnknown::QueryInterface} $0 '("${IID_IPropertyStore}",.r2)'
      ${If} $2 P<> 0
        System::Call 'Oleaut32::SysAllocString(w "${BUNDLEID}") i.r3'
        System::Call '*${SYSSTRUCT_PROPERTYKEY}(${PKEY_AppUserModel_ID})p.r4'
        System::Call '*${SYSSTRUCT_PROPVARIANT}(${VT_BSTR},,&i4 $3)p.r5'
        ${IPropertyStore::SetValue} $2 '($4,$5)'

        System::Call 'Oleaut32::SysFreeString($3)'
        System::Free $4
        System::Free $5
        ${IPropertyStore::Commit} $2 ""
        ${IUnknown::Release} $2 ""
        ${IPersistFile::Save} $1 '("${shortcut}",1)'
      ${EndIf}
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
!macroend

Function CreateDesktopShortcut
  CreateShortcut "$DESKTOP\${MAINBINARYNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro SetLnkAppUserModelId "$DESKTOP\${MAINBINARYNAME}.lnk"
FunctionEnd

Function CreateStartMenuShortcut
  CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
  CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\${MAINBINARYNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\$AppStartMenuFolder\${MAINBINARYNAME}.lnk"
FunctionEnd

; =====================================================================
; QU ADDITIONS -- everything below this line is Qu's, not Tauri's.
; =====================================================================

; Build-time payloads staged by release.yml (absent in a plain local build:
; each component below is then simply not compiled in).
!define QU_EXTRAS "$%QU_STUDIO_EXTRAS%"
!if /FileExists "${QU_EXTRAS}\qu-jupyter.exe"
  !define QU_HAVE_JUPYTER
!endif
!if /FileExists "${QU_EXTRAS}\docs\index.html"
  !define QU_HAVE_DOCS
!endif
!if /FileExists "${QU_EXTRAS}\path-helper.ps1"
  !define QU_HAVE_PATH_HELPER
!endif
;
; ---------------------------------------------------------------------
; PATH (current user only: HKCU\Environment\Path, no elevation needed)
; ---------------------------------------------------------------------
;
; Edited by installer/windows/path-helper.ps1 -- the same script, with the
; same tests, the CLI installer uses -- never with NSIS string code: NSIS
; strings stop at 1024 characters and a longer PATH would be written back
; truncated. release.yml stages the script in $%QU_STUDIO_EXTRAS%; it is
; installed next to qu.exe so the uninstaller can run it too.
;
; (Until 0.4.6 this was done in-process through the System plug-in. It
; never changed PATH on a real Windows install -- installer-verify-studio
; failed on it from its first run -- and logged nothing a silent install
; shows, so it was replaced rather than debugged further.)

!macro QU_FIND_POWERSHELL
  ; 64-bit PowerShell from this 32-bit installer (SysNative bypasses the
  ; WOW64 redirect); otherwise whatever System32 resolves to.
  ${If} ${FileExists} "$WINDIR\SysNative\WindowsPowerShell\v1.0\powershell.exe"
    StrCpy $R8 "$WINDIR\SysNative\WindowsPowerShell\v1.0\powershell.exe"
  ${Else}
    StrCpy $R8 "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  ${EndIf}
!macroend

; Exit code in $R9: 0 changed, 10 nothing to do, anything else failed.
!macro QU_RUN_PATH_HELPER ACTION
  !insertmacro QU_FIND_POWERSHELL
  nsExec::ExecToLog '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\path-helper.ps1" -Action ${ACTION} -Dir "$INSTDIR" -Scope User'
  Pop $R9
  DetailPrint "Qu: user PATH ${ACTION} $INSTDIR: helper exit code $R9"
!macroend

; ---------------------------------------------------------------------
; Editor integrations: best-effort and silent. Each editor is detected by
; its real per-user config folder (created the first time that editor has
; actually run -- not a registry probe, which is version- and install-
; type-specific), and gets the exact files, at the exact destination, that
; its own editors/<name>-qu/README.md documents as the manual install. If
; the folder is absent the step is simply skipped; nothing here can fail
; the install. Not removed on uninstall on purpose: they are small,
; standalone files the user may still want (syntax highlighting works with
; the engine gone), and Sublime/Notepad++ have no clean uninstall concept.
;
; Where the files are: tauri.conf.json bundles "../../editors/**/*" as
; resources, and Tauri maps each "../" in a resource path to "_up_", so
; they land in $INSTDIR\_up_\_up_\editors\ -- NOT $INSTDIR\resources\,
; which is what the old, never-executed installer.nsh assumed.
; ---------------------------------------------------------------------
!define QU_EDITORS_DIR "$INSTDIR\_up_\_up_\editors"

; (QuInstallEditorIntegrations became the optional "Editor plugins" sections below.)

; ---------------------------------------------------------------------
; Entry points, called from the stock sections above. Under Tauri 2 these
; become the bodies of NSIS_HOOK_POSTINSTALL / NSIS_HOOK_PREUNINSTALL.
; ---------------------------------------------------------------------
Function QuPostInstall
  ; PATH moved to the optional "Add qu to the PATH" section below (0.4.6).
FunctionEnd

; ---------------------------------------------------------------------
; Optional components. Sections run in file order, so these run after the
; stock (hidden) Install section has put qu.exe and the editor resources
; in $INSTDIR. What was installed is recorded under the uninstall key, and
; the uninstaller removes exactly that.
; ---------------------------------------------------------------------

!macro QU_KERNEL_ACTION ACTION
  StrCpy $R6 ""
  !if "${INSTALLMODE}" == "perMachine"
    StrCpy $R6 " --system"
  !else if "${INSTALLMODE}" == "both"
    ${If} $MultiUser.InstallMode == "AllUsers"
      StrCpy $R6 " --system"
    ${EndIf}
  !endif
  nsExec::ExecToLog '"$INSTDIR\qu-jupyter.exe" ${ACTION}$R6'
  Pop $R7
!macroend

!ifdef QU_HAVE_PATH_HELPER
; On by default; unticked (or /NOPATH) leaves PATH alone.
Section "Add qu to the PATH" SecQuPath
  SetOutPath "$INSTDIR"
  File "${QU_EXTRAS}\path-helper.ps1"
  !insertmacro QU_RUN_PATH_HELPER Add
  ${If} $R9 != 0
  ${AndIf} $R9 != 10
    MessageBox MB_ICONEXCLAMATION "Qu Studio was installed, but adding qu to the PATH failed ($R9). Add $INSTDIR to your PATH manually." /SD IDOK
  ${EndIf}
SectionEnd
!else
  !warning "QU_STUDIO_EXTRAS has no path-helper.ps1: this installer will not offer to put qu on the PATH"
!endif

!ifdef QU_HAVE_JUPYTER
Section "Jupyter kernel (Qu in JupyterLab, Notebook, VS Code)" SecQuJupyter
  SetOutPath "$INSTDIR"
  File "${QU_EXTRAS}\qu-jupyter.exe"
  File "${QU_EXTRAS}\qu-jupyter-start.cmd"
  !insertmacro QU_KERNEL_ACTION install
  ${If} $R7 == 0
    WriteRegDWORD SHCTX "${UNINSTKEY}" "QuJupyter" 1
    DetailPrint "Qu: Jupyter kernel registered"
  ${Else}
    DetailPrint "Qu: registering the Jupyter kernel failed (exit code $R7)"
  ${EndIf}
SectionEnd
!endif

!ifdef QU_HAVE_DOCS
Section /o "Offline documentation (about 25 MB)" SecQuDocs
  SetOutPath "$INSTDIR\docs"
  File /r "${QU_EXTRAS}\docs\*.*"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "QuDocs" 1
SectionEnd
!endif

; Per-user editor folders: an editor reads plugins from the profile of
; whoever runs it, whatever the install mode.
SectionGroup "Editor plugins" SecQuPlugins
  Section "VS Code" SecQuVSCode
    ; folder name as editors/vscode-qu/README.md "Option A" gives it
    ; (package.json name-version). Bump with editors/vscode-qu/package.json.
    StrCpy $R5 "$PROFILE\.vscode\extensions\qu-language-0.1.0"
    CreateDirectory $R5
    CopyFiles /SILENT "${QU_EDITORS_DIR}\vscode-qu\*.*" $R5
    WriteRegStr SHCTX "${UNINSTKEY}" "QuVSCode" $R5
  SectionEnd
  Section "Sublime Text" SecQuSublime
    ${If} ${FileExists} "$APPDATA\Sublime Text 3\Packages\*.*"
    ${AndIfNot} ${FileExists} "$APPDATA\Sublime Text\Packages\*.*"
      StrCpy $R5 "$APPDATA\Sublime Text 3\Packages\Qu"
    ${Else}
      StrCpy $R5 "$APPDATA\Sublime Text\Packages\Qu"
    ${EndIf}
    CreateDirectory $R5
    CopyFiles /SILENT "${QU_EDITORS_DIR}\sublime-qu\Qu.sublime-syntax" $R5
    CopyFiles /SILENT "${QU_EDITORS_DIR}\sublime-qu\Qu.sublime-build" $R5
    WriteRegStr SHCTX "${UNINSTKEY}" "QuSublime" $R5
  SectionEnd
  Section "Notepad++" SecQuNpp
    CreateDirectory "$APPDATA\Notepad++\userDefineLangs"
    CopyFiles /SILENT "${QU_EDITORS_DIR}\notepadpp-qu\Qu.udl.xml" "$APPDATA\Notepad++\userDefineLangs"
    WriteRegStr SHCTX "${UNINSTKEY}" "QuNotepadpp" "$APPDATA\Notepad++\userDefineLangs\Qu.udl.xml"
  SectionEnd
SectionGroupEnd

; Shortcuts beside Qu Studio's own, wherever the stock code put that one
; (the Start-menu page's folder, or none if the user declined).
Section -QuShortcuts
  ${If} ${FileExists} "$SMPROGRAMS\$AppStartMenuFolder\${MAINBINARYNAME}.lnk"
    CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\Qu CLI (REPL).lnk" "$INSTDIR\qu.exe" "repl" "$INSTDIR\qu.exe" 0
    !ifdef QU_HAVE_JUPYTER
      ${If} ${SectionIsSelected} ${SecQuJupyter}
        CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\Start Jupyter (Qu).lnk" "$INSTDIR\qu-jupyter-start.cmd" "" "$INSTDIR\qu.exe" 0
      ${EndIf}
    !endif
    !ifdef QU_HAVE_DOCS
      ${If} ${SectionIsSelected} ${SecQuDocs}
        CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\Qu Documentation.lnk" "$INSTDIR\docs\index.html"
      ${EndIf}
    !endif
  ${EndIf}
  ${If} ${FileExists} "$DESKTOP\${MAINBINARYNAME}.lnk"
    CreateShortcut "$DESKTOP\Qu CLI (REPL).lnk" "$INSTDIR\qu.exe" "repl" "$INSTDIR\qu.exe" 0
    !ifdef QU_HAVE_JUPYTER
      ${If} ${SectionIsSelected} ${SecQuJupyter}
        CreateShortcut "$DESKTOP\Start Jupyter (Qu).lnk" "$INSTDIR\qu-jupyter-start.cmd" "" "$INSTDIR\qu.exe" 0
      ${EndIf}
    !endif
  ${EndIf}
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !ifdef QU_HAVE_PATH_HELPER
    !insertmacro MUI_DESCRIPTION_TEXT ${SecQuPath} "Lets any new terminal run the bundled qu command by name. Only this one entry is added to your user PATH; uninstalling removes it."
  !endif
  !ifdef QU_HAVE_JUPYTER
    !insertmacro MUI_DESCRIPTION_TEXT ${SecQuJupyter} "qu-jupyter.exe, registered as the $\"Qu$\" kernel for JupyterLab, Notebook and VS Code's Jupyter extension, and a Start Jupyter (Qu) shortcut. Jupyter itself is installed separately (pip install jupyterlab)."
  !endif
  !ifdef QU_HAVE_DOCS
    !insertmacro MUI_DESCRIPTION_TEXT ${SecQuDocs} "The full reference and guides as local HTML; help(name) then opens the local page."
  !endif
  !insertmacro MUI_DESCRIPTION_TEXT ${SecQuPlugins} "Syntax highlighting and run commands for Qu files. Each is preselected only if that editor is installed."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Function QuComponentDefaults
  ${IfNot} ${FileExists} "$PROFILE\.vscode\*.*"
    !insertmacro UnselectSection ${SecQuVSCode}
  ${EndIf}
  ${IfNot} ${FileExists} "$APPDATA\Sublime Text\Packages\*.*"
  ${AndIfNot} ${FileExists} "$APPDATA\Sublime Text 3\Packages\*.*"
    !insertmacro UnselectSection ${SecQuSublime}
  ${EndIf}
  ${IfNot} ${FileExists} "$APPDATA\Notepad++\*.*"
    !insertmacro UnselectSection ${SecQuNpp}
  ${EndIf}
  ${GetParameters} $R0
  !ifdef QU_HAVE_DOCS
    ClearErrors
    ${GetOptions} $R0 "/WITHDOCS" $R1
    ${IfNot} ${Errors}
      !insertmacro SelectSection ${SecQuDocs}
    ${EndIf}
  !endif
  !ifdef QU_HAVE_JUPYTER
    ClearErrors
    ${GetOptions} $R0 "/NOJUPYTER" $R1
    ${IfNot} ${Errors}
      !insertmacro UnselectSection ${SecQuJupyter}
    ${EndIf}
  !endif
  !ifdef QU_HAVE_PATH_HELPER
    ClearErrors
    ${GetOptions} $R0 "/NOPATH" $R1
    ${IfNot} ${Errors}
      !insertmacro UnselectSection ${SecQuPath}
    ${EndIf}
  !endif
  ClearErrors
  ${GetOptions} $R0 "/NOPLUGINS" $R1
  ${IfNot} ${Errors}
    !insertmacro UnselectSection ${SecQuVSCode}
    !insertmacro UnselectSection ${SecQuSublime}
    !insertmacro UnselectSection ${SecQuNpp}
  ${EndIf}
FunctionEnd

Function un.QuPreUninstall
  ${If} ${FileExists} "$INSTDIR\path-helper.ps1"
    !insertmacro QU_RUN_PATH_HELPER Remove
    Delete "$INSTDIR\path-helper.ps1"
  ${EndIf}
  ; Optional components: exactly what the install recorded.
  ReadRegDWORD $R0 SHCTX "${UNINSTKEY}" "QuJupyter"
  ${If} $R0 == 1
  ${AndIf} ${FileExists} "$INSTDIR\qu-jupyter.exe"
    !insertmacro QU_KERNEL_ACTION uninstall
  ${EndIf}
  Delete "$INSTDIR\qu-jupyter.exe"
  Delete "$INSTDIR\qu-jupyter-start.cmd"
  ReadRegDWORD $R0 SHCTX "${UNINSTKEY}" "QuDocs"
  ${If} $R0 == 1
    RMDir /r "$INSTDIR\docs"
  ${EndIf}
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" "QuVSCode"
  ${If} $R0 != ""
    RMDir /r $R0
  ${EndIf}
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" "QuSublime"
  ${If} $R0 != ""
    Delete "$R0\Qu.sublime-syntax"
    Delete "$R0\Qu.sublime-build"
    RMDir $R0
  ${EndIf}
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" "QuNotepadpp"
  ${If} $R0 != ""
    Delete $R0
  ${EndIf}
  ; the stock code reads the folder after this runs; read it here too
  !insertmacro MUI_STARTMENU_GETFOLDER Application $AppStartMenuFolder
  Delete "$SMPROGRAMS\$AppStartMenuFolder\Qu CLI (REPL).lnk"
  Delete "$SMPROGRAMS\$AppStartMenuFolder\Start Jupyter (Qu).lnk"
  Delete "$SMPROGRAMS\$AppStartMenuFolder\Qu Documentation.lnk"
  Delete "$DESKTOP\Qu CLI (REPL).lnk"
  Delete "$DESKTOP\Start Jupyter (Qu).lnk"
FunctionEnd
