# Application icons

No placeholder bitmap is committed. The build script creates an ignored
`icon.ico` only when Tauri needs one for a development build; `lifecycle.rs`
generates the matching tray pixels at runtime. The source tree therefore
contains no misleading release artwork or binary placeholder.

Before a signed release candidate, export the approved mark as Windows ICO and
installer PNG assets, record its source/provenance, add the paths to
`tauri.conf.json`, and verify 100%, 150%, 200%, high-contrast, light-taskbar, and
dark-taskbar renderings. An unsigned or generic Tauri icon must not ship.
