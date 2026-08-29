# Interactive LLM Powered NPCs 2.0 — local-review handoff

Updated: 2026-08-29

## Current state

- Branch: `feat/2.0-overhaul`.
- The 1.x notebook/SadTalker prototype has been retired. The active product is
  a Windows-only, generic external-capture companion: no game injection, mod,
  hook, native-rig, or executable adapter path is supported.
- Conversation is API-first. Local model downloads are optional and limited by
  policy to explicitly chosen generic lip-sync packs.
- Named provider/model loadouts resolve global → game → character settings,
  pin every turn's route, and permit only explicit manual recovery choices.
- The visual model catalog is research/qualification data; no third-party
  talking-head model weights are installed or bundled.

## Local test artifact

The current unsigned Debug test installer is under:

`artifacts/test-app/<UTC timestamp>/`

It is always an `unchecked-development-package`, local-review-only and not an
RC. The latest installer smoke result is
`artifacts/installer-smoke-final.json`; it verifies the installed runtime and
media broker, private AppData/config DACLs, process supervision, normal
uninstall, and no residue.

## Verified locally

- Root Rust workspace tests, formatting, and strict Clippy.
- Tauri tests, provider-loadout persistence tests, frontend tests/typecheck,
  production build, and rendered visual evidence.
- 20 profile/replay validations, 7 deterministic simulation scenarios, worker
  protocol checks, Windows media-broker CTest, and inert game-load harness.
- Current-tree secret scan, strict license/provenance policy, deterministic
  SBOM/source evidence, docs links, and Debug installer smoke.

## Remaining release gates

- The reachable Git history still contains the historical `apikeys.json`
  filename. Do not rewrite history without explicit user approval.
- Production TUF roots/catalog signatures, signing custody, approved model
  pack runner and qualified pack metadata are not provisioned.
- No real generic lip-sync pack, live-game certification, clean Windows VM
  matrix, display/DPI/HDR matrix, live latency/FPS/WER/soak benchmark, or
  public release has been completed.

## Operating constraints

- Do not push, tag, publish, upload, activate updates, or release without
  explicit user approval.
- Keep Windows power settings unchanged. If a future GPU workload is needed,
  set `C:\Users\akshi\Desktop\Code Palace\gpu use.txt` to `yes` immediately
  before it and restore `no` afterward. The current value is `no`.
- Prefer task-owned GUI windows on a secondary display when Windows exposes
  one; otherwise use the active display without moving unrelated windows.
