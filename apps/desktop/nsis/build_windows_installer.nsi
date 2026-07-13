; FreeDB Windows NSIS Installer Script
; Build from project root: makensis apps/desktop/nsis/build_windows_installer.nsi

;--------------------------------
; Unicode support (MUST be before includes)

Unicode true

;--------------------------------
; Metadata

!define PRODUCT_NAME "FreeDB"
!define PRODUCT_VERSION "1.1.0"
!define PRODUCT_PUBLISHER "FreeDB Contributors"
!define PRODUCT_WEB_SITE "https://github.com/fudongri/freedb"
!define PRODUCT_DIR_REGKEY "Software\Microsoft\Windows\CurrentVersion\App Paths\freedb.exe"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define PRODUCT_UNINST_ROOT_KEY "HKLM"

SetCompressor lzma
SetCompressorDictSize 32

;--------------------------------
; MUI 2 modern interface

!include "MUI2.nsh"
!include "FileFunc.nsh"

;--------------------------------
; General

Name "${PRODUCT_NAME} ${PRODUCT_VERSION}"
OutFile "..\..\..\target\FreeDB-${PRODUCT_VERSION}-x86_64-setup.exe"
InstallDir "$PROGRAMFILES64\${PRODUCT_NAME}"
InstallDirRegKey HKLM "${PRODUCT_UNINST_KEY}" "InstallLocation"
RequestExecutionLevel admin
BrandingText "${PRODUCT_PUBLISHER}"

;--------------------------------
; Interface settings

!define MUI_ABORTWARNING
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_BITMAP "header.bmp"
!define MUI_HEADERIMAGE_RIGHT

;--------------------------------
; Pages

; License page
!insertmacro MUI_PAGE_LICENSE "LICENSE.txt"

; Install directory page (user can change path)
!insertmacro MUI_PAGE_DIRECTORY

; Install progress page
!insertmacro MUI_PAGE_INSTFILES

; Uninstall confirm page
!insertmacro MUI_UNPAGE_CONFIRM

; Uninstall progress page
!insertmacro MUI_UNPAGE_INSTFILES

;--------------------------------
; Languages — English + Chinese with proper font handling

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

;--------------------------------
; Installer Section

Section "FreeDB" SecMain
    SetOutPath "$INSTDIR"

    ; Main executable
    File "/oname=freedb.exe" "..\..\..\target\x86_64-pc-windows-gnu\release\freedb.exe"

    ; Icon (used by shortcuts)
    File "/oname=freedb-icon.ico" "..\assets\freedb-icon.ico"

    ; License
    File "/oname=LICENSE.txt" "LICENSE.txt"

    ; Create start menu entry
    CreateDirectory "$SMPROGRAMS\${PRODUCT_NAME}"
    CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\FreeDB.lnk" "$INSTDIR\freedb.exe" "" "$INSTDIR\freedb-icon.ico"
    CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall.lnk" "$INSTDIR\uninst.exe"

    ; Create desktop shortcut
    CreateShortCut "$DESKTOP\FreeDB.lnk" "$INSTDIR\freedb.exe" "" "$INSTDIR\freedb-icon.ico"

    ; Write uninstaller
    WriteUninstaller "$INSTDIR\uninst.exe"

    ; Registry for Add/Remove Programs
    WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "DisplayName" "${PRODUCT_NAME}"
    WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\uninst.exe"
    WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\freedb-icon.ico"
    WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
    WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
    WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
    WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegDWORD HKLM "${PRODUCT_UNINST_KEY}" "NoModify" 1
    WriteRegDWORD HKLM "${PRODUCT_UNINST_KEY}" "NoRepair" 1

    ; Estimate installed size
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD HKLM "${PRODUCT_UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

;--------------------------------
; Uninstaller Section

Section "Uninstall"
    ; Remove files
    Delete "$INSTDIR\freedb.exe"
    Delete "$INSTDIR\freedb-icon.ico"
    Delete "$INSTDIR\LICENSE.txt"
    Delete "$INSTDIR\uninst.exe"
    RMDir "$INSTDIR"

    ; Remove shortcuts
    Delete "$SMPROGRAMS\${PRODUCT_NAME}\FreeDB.lnk"
    Delete "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall.lnk"
    RMDir "$SMPROGRAMS\${PRODUCT_NAME}"
    Delete "$DESKTOP\FreeDB.lnk"

    ; Remove registry entries
    DeleteRegKey HKLM "${PRODUCT_UNINST_KEY}"
    DeleteRegKey HKLM "${PRODUCT_DIR_REGKEY}"

    SetAutoClose true
SectionEnd
