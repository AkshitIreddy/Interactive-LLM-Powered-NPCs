# Generated project sidecars

The Tauri bundle expects these ignored, target-suffixed build products:

- `npc-runtime-x86_64-pc-windows-msvc.exe`
- `npc-media-broker-x86_64-pc-windows-msvc.exe`

`scripts/prepare-sidecars.ps1` builds and copies only project-owned binaries.
Local AI runtimes and model weights are never placed here; the Model Manager
installs separately verified runtime/model packs after onboarding.
