; Pinned-offline Tauri NSIS hooks.
; Keep installation current-user and do not add services, drivers, firewall rules,
; scheduled tasks, machine-wide PATH changes, or updater activation here.

!ifndef NPC_WEBVIEW2_OFFLINE_INSTALLER_PATH
  !error "NPC_WEBVIEW2_OFFLINE_INSTALLER_PATH must be supplied by the fail-closed package stage"
!endif

!ifndef NPC_WEBVIEW2_OFFLINE_INSTALLER_SHA256
  !error "NPC_WEBVIEW2_OFFLINE_INSTALLER_SHA256 must be supplied by the fail-closed package stage"
!endif

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Preparing the Interactive NPCs 2.0 local installation"
  ; Tauri's built-in offlineInstaller mode performs an unconditional HEAD to
  ; a mutable fwlink before consulting its cache. The product config therefore
  ; uses `skip`, while the package stage supplies exact locally verified bytes
  ; to this hook. The File instruction fails the build if those bytes vanish.
  Push $R8
  Push $R9
  ReadRegStr $R8 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  StrCmp $R8 "" 0 npc2_webview2_offline_done
  ReadRegStr $R8 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  StrCmp $R8 "" 0 npc2_webview2_offline_done
  ReadRegStr $R8 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  StrCmp $R8 "" 0 npc2_webview2_offline_done
  InitPluginsDir
  Delete "$PLUGINSDIR\MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
  File "/oname=$PLUGINSDIR\MicrosoftEdgeWebView2RuntimeInstallerX64.exe" "${NPC_WEBVIEW2_OFFLINE_INSTALLER_PATH}"
  DetailPrint "Installing the exact pinned Microsoft WebView2 Evergreen runtime"
  ExecWait '"$PLUGINSDIR\MicrosoftEdgeWebView2RuntimeInstallerX64.exe" /silent /install' $R9
  StrCmp $R9 0 npc2_webview2_offline_installed
  Abort "Pinned Microsoft WebView2 installation failed with exit code $R9"
npc2_webview2_offline_installed:
  Delete "$PLUGINSDIR\MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
npc2_webview2_offline_done:
  Pop $R9
  Pop $R8
!macroend

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Installation complete; model packs remain opt-in downloads"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing application files; user data is preserved"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Tauri records $INSTDIR as the default value beneath this exact
  ; current-user manufacturer/product key. Remove only the value owned by
  ; this uninstall instance; preserve a mismatched key and all unrelated data.
  Push $R8
  ReadRegStr $R8 HKCU "Software\github\Interactive NPCs Response Console" ""
  StrCmp $R8 "$INSTDIR" 0 npc2_postuninstall_registry_done
  DeleteRegValue HKCU "Software\github\Interactive NPCs Response Console" ""
  DeleteRegKey /ifempty HKCU "Software\github\Interactive NPCs Response Console"
  DeleteRegKey /ifempty HKCU "Software\github"
npc2_postuninstall_registry_done:
  Pop $R8
  DetailPrint "Application removal complete"
!macroend
