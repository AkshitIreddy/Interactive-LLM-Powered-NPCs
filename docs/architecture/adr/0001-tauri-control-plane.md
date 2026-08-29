# ADR-0001: Use Tauri 2 as the control plane

Status: Accepted with an architecture-spike gate  
Date: 2026-08-28

## Context

The consumer application needs polished onboarding/configuration, low idle overhead, Windows packaging/updating, tray lifecycle and access to a Rust runtime. It must also capture/render/audio-process beside a game. Putting realtime video or PCM in a WebView would introduce copies, scheduling jitter and a larger failure surface.

## Decision

Use Tauri 2, React/TypeScript, Vite and React Aria Components for the Response Console. Treat it as a control plane: commands and ordered channels carry bounded configuration/status only. C++/WinRT owns realtime media and D3D presentation; Rust owns orchestration and provider/state policy.

Use WebView2 Evergreen and ship NSIS for the Windows user path. Signed update artifacts may be built/tested locally, but the update feed remains inactive until explicit release approval.

## Consequences

- Rich accessible UI can iterate independently of realtime media.
- Rust commands expose a narrow bridge; CSP and navigation policy remain tractable.
- WebView2 availability/version must be checked and explained by installer diagnostics.
- UI crash/reload must reconnect without corrupting the runtime session.
- There are three languages/toolchains, justified by platform boundaries rather than duplicated business logic.

## Validation and fallback

An isolated release-build spike must measure minimized/active UI overhead beside synthetic game load, ordered channel recovery, clean runtime restart and Windows 10/11 packaging. Reject Tauri if minimized release overhead materially violates the frame budget, recovery cannot be made reliable, or non-admin packaging fails. The predefined shell fallback is WinUI 3; Rust contracts and native broker remain unchanged.

## Evidence

- Tauri architecture: <https://v2.tauri.app/concept/architecture/>
- Tauri channels: <https://v2.tauri.app/develop/calling-frontend/#channels>
- Tauri Windows installer: <https://v2.tauri.app/distribute/windows-installer/>
- Tauri updater/signatures: <https://v2.tauri.app/plugin/updater/>

