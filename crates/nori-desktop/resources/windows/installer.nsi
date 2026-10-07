; nori's Windows installer (scripts/bundle-windows.sh runs makensis on it):
;   makensis /DVERSION=0.1.0 /DSRC=<folder with nori.exe…> /DOUTFILE=<setup.exe> installer.nsi
;
; Per-user install into %LOCALAPPDATA%\nori, a Start menu shortcut, and .nori files opening in
; nori. `/P` skips the pages (passive, for an updater), `/R` starts nori when done; /S is NSIS's
; silent mode.

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define PRODUCT "nori"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"
!define PROGID "nori.document"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\${PRODUCT}"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
BrandingText "nori ${VERSION} · lsuite"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "nori installer"
VIAddVersionKey "LegalCopyright" "MIT licence"

!define MUI_ICON "${SRC}\nori.ico"
!define MUI_UNICON "${SRC}\nori.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\nori.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open nori"

Var Passive

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipWhenPassive
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipWhenPassive
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  StrCpy $Passive 0
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/P" $1
  ${IfNot} ${Errors}
    StrCpy $Passive 1
    SetAutoClose true
  ${EndIf}
FunctionEnd

Function SkipWhenPassive
  ${If} $Passive == 1
    Abort
  ${EndIf}
FunctionEnd

Section "nori" Main
  SetOutPath "$INSTDIR"
  ; nori may still be closing (an updater quits it just before running this).
  Sleep 500
  SetOverwrite on
  File "${SRC}\nori.exe"
  File "${SRC}\nori-cli.exe"
  File "${SRC}\nori-mcp.exe"
  File "${SRC}\nori.ico"
  File "${SRC}\LICENSE.txt"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\nori.exe" "" "$INSTDIR\nori.ico" 0

  ; .nori documents open in nori; Word, Excel, PowerPoint, OpenDocument, CSV and Markdown files
  ; list it under "Open with".
  WriteRegStr HKCU "Software\Classes\.nori" "" "${PROGID}"
  WriteRegStr HKCU "Software\Classes\.nori" "Content Type" "application/vnd.lsuite.nori"
  WriteRegStr HKCU "Software\Classes\${PROGID}" "" "nori document"
  WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" "$\"$INSTDIR\nori.ico$\""
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" "$\"$INSTDIR\nori.exe$\" $\"%1$\""
  WriteRegStr HKCU "Software\Classes\Applications\nori.exe\shell\open\command" "" "$\"$INSTDIR\nori.exe$\" $\"%1$\""
  !macro OpenWith ext
    WriteRegStr HKCU "Software\Classes\Applications\nori.exe\SupportedTypes" "${ext}" ""
    WriteRegStr HKCU "Software\Classes\${ext}\OpenWithList\nori.exe" "" ""
  !macroend
  !insertmacro OpenWith ".nori"
  !insertmacro OpenWith ".docx"
  !insertmacro OpenWith ".xlsx"
  !insertmacro OpenWith ".pptx"
  !insertmacro OpenWith ".odt"
  !insertmacro OpenWith ".ods"
  !insertmacro OpenWith ".odp"
  !insertmacro OpenWith ".csv"
  !insertmacro OpenWith ".md"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\nori.ico$\""
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "lsuite"
  WriteRegStr HKCU "${UNINSTKEY}" "URLInfoAbout" "https://lsuite.xyz/nori"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" "$\"$INSTDIR\uninstall.exe$\" /S"
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" $0

  ; /R (from an updater): start nori again.
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/R" $1
  ${IfNot} ${Errors}
    Exec '"$INSTDIR\nori.exe"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\nori.exe"
  Delete "$INSTDIR\nori-cli.exe"
  Delete "$INSTDIR\nori-mcp.exe"
  Delete "$INSTDIR\nori.ico"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${UNINSTKEY}"
  ; The file associations this installer wrote; documents and settings (%APPDATA%\nori) stay.
  ReadRegStr $0 HKCU "Software\Classes\.nori" ""
  ${If} $0 == "${PROGID}"
    DeleteRegKey HKCU "Software\Classes\.nori"
  ${EndIf}
  DeleteRegKey HKCU "Software\Classes\${PROGID}"
  DeleteRegKey HKCU "Software\Classes\Applications\nori.exe"
  !macro UnOpenWith ext
    DeleteRegKey HKCU "Software\Classes\${ext}\OpenWithList\nori.exe"
  !macroend
  !insertmacro UnOpenWith ".docx"
  !insertmacro UnOpenWith ".xlsx"
  !insertmacro UnOpenWith ".pptx"
  !insertmacro UnOpenWith ".odt"
  !insertmacro UnOpenWith ".ods"
  !insertmacro UnOpenWith ".odp"
  !insertmacro UnOpenWith ".csv"
  !insertmacro UnOpenWith ".md"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd
