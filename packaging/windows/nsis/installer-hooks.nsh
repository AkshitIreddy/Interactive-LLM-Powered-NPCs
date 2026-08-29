; Intentionally minimal Tauri NSIS hooks.
; Keep installation current-user and do not add services, drivers, firewall rules,
; scheduled tasks, machine-wide PATH changes, or updater activation here.

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Preparing the Interactive NPCs 2.0 local installation"
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
