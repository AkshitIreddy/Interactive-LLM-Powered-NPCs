# See me — paused handoff

Paused at the user’s request on 2026-08-29. Do not push, publish, tag, upload,
activate an updater, use provider credentials, download models, or change the
power profile while resuming this work.

## What is complete

- The 2.0 local Debug review build was previously packaged through its full
  no-skip gate from the clean local sanitized source and installed into the
  task-owned hands-on location:
  `C:\Users\akshi\AppData\Local\InteractiveNPCsHandsOnTest\app`.
- That previous strict package passed lint, tests, security/history scans,
  license/SBOM checks, installer smoke, authenticated shell-to-runtime/broker
  supervision, 20 profile validation, and a final installed runtime doctor.
- The prior walkthrough video was audited and found **not** to be a continuous
  test-game conversation. Do not present it as one. It is capture/runtime
  evidence only:
  `artifacts/actual-ui-demo/actual-synthetic-capture-walkthrough.mp4`.
- Adversarial pass 1 found a concrete mismatch: the UI showed Mara Venn and a
  lighthouse question while the native fixture received a different transcript
  and returned a generic “Hold Resident” response.
- Commit `ef0c787 fix(simulation): align Eclipse Harbor fixture turn` corrects
  that mismatch. The synthetic Eclipse Harbor route now uses the safe generic
  game boundary, passes the exact displayed lighthouse prompt, selects Mara
  Venn, returns a matching three-sentence deterministic reply, and paces only
  deterministic fixture stages for readable recording. It does **not** claim a
  live LLM, STT, TTS, audio, or lip-sync result.
- Validation already completed for that commit:
  - Tauri control library: 56/56 passed
  - Runtime-host integration: 7/7 passed
  - Windows TypeScript typecheck: passed
  - Rust formatting: passed
- The current adversarial ledger is at:
  `artifacts/actual-ui-demo/ADVERSARIAL_REFINEMENT_LEDGER.md`.

## Current strict-gate blocker

The first strict package attempt after `ef0c787` correctly failed in nested
Tauri Clippy, before any new installer was staged:

```text
clippy::large_enum_variant
WireRequest::SimulateTurn(NativeSimulationRequest)
```

The failed output is:

`artifacts/actual-ui-demo/fixture-v4-strict-package.stdout.log`

The failure happened because `NativeSimulationRequest` gained the explicit
generic fixture selection. The prior hands-on app has not been replaced.

## In-progress uncommitted fix — preserve it

An active lane was deliberately interrupted for this pause. Its narrowly scoped
uncommitted work is in:

`apps/control/src-tauri/src/sidecar_protocol.rs`

It boxes `WireRequest::SimulateTurn` and adds a JSON wire-shape regression test,
which preserves the external protocol rather than adding an allow/suppression.
Formatting passed, but its final focused test, Clippy, and commit were blocked
by a transient host-wide `No file descriptors available (os error 24)` failure.
Do **not** discard or overwrite this edit when resuming.

## Clean release-source state

The disposable sanitized local release source is:

`artifacts/local-sanitized-release-source/tree-v3`

It includes the conversation-alignment change as commit `9613503`, but does
not yet include the uncommitted Box fix. It is intentionally separate so the
old repository history is not rewritten. Its older successful strict package
remains under `artifacts/local-sanitized-release-source/packages-strict-20260829T123116Z`.

## Next safe steps

1. Wait for the host process-limit issue to clear. Inspect `git status` and the
   `sidecar_protocol.rs` change; ensure no unrelated files are staged.
2. Run the focused nested Tauri tests and strict Clippy. Confirm the new Box
   keeps the serialized `simulate_turn` JSON shape unchanged. Run the Windows
   TypeScript typecheck again if practical.
3. Commit the Box fix atomically, then apply that commit into
   `artifacts/local-sanitized-release-source/tree-v3`.
4. Run a new **no-skip** strict package from that clean sanitized source. Do
   not use `-SkipChecks` as a substitute. Confirm the manifest reports all
   five security gates true and `local-review-only`/updater disabled.
5. Use the guarded installer-smoke process against the new installer, then
   install that exact package into the task-owned HandsOnTest location. Preserve
   `%APPDATA%\io.github.akshitireddy.interactive-npcs`, Credential Manager,
   existing debug replay metadata, and prior doctor data.
6. Record a new continuous, truthful synthetic-game conversation proof:
   - launch the original CPU-decoded Eclipse Harbor synthetic target;
   - use the currently available primary display (`DISPLAY1`) only if no
     secondary display exists;
   - show real WGC target/frame diagnostics, the readable native deterministic
     Response Spine, the exact player prompt, Mara’s matching native reply,
     and the Conversation ledger;
   - record with CPU FFmpeg/GDI capture and no GPU model inference;
   - label the result as a synthetic test game plus native deterministic
     fixture, with silent/no-lip-sync limitations visible.
7. Inspect the new video frame-by-frame (full frame plus readable close-ups),
   then commission a fresh blind critique. Continue the adversarial refinement
   loop rather than treating one green recording as completion.

## Important boundaries

- The GPU coordination file last remained `no`; no model inference was run.
- The first display is available for task windows; do not move unrelated user
  windows or open visible terminals.
- The current synthetic test cannot honestly be called live gameplay, live
  microphone/STT, live cloud inference, audible TTS, or lip-sync. Keep those
  claims out of the new video until separately proven.

## Cleanup completed after this handoff was created

The user explicitly requested a disk cleanup before work resumed. The following
confirmed rebuildable/inactive material was permanently removed after live-use
checks; it is recoverable by rebuilding or downloading from the documented
upstream sources, not from the Recycle Bin.

- `C:\Users\akshi\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\InteractiveNPCsResearch`
  — 21,107,522,025 bytes of rejected MuseTalk qualification environments,
  duplicated weights, and test output. Hash-verified evidence remains under
  `artifacts/real-weight-qualification`.
- 56 inactive `C:\Users\akshi\AppData\Local\Temp\_MEI*` PyInstaller
  extraction folders — 34,427,387,052 bytes at the final pre-delete snapshot.
  A transient icon lock was retried safely; final remaining `_MEI*` count was
  zero.
- 26 repository-local generated targets and stale copies: root/nested/per-crate
  Cargo targets, native build outputs, demo render scratch space, obsolete test
  installers, old sanitized source trees/tar snapshots, empty package folders,
  and the failed strict-package directory. This removed roughly 51 GB of
  reproducible build/cache output according to the pre-delete inventory.

Preserved deliberately: tracked source and `.git`, `tree-v3`, the last
successful strict package, `artifacts/actual-ui-demo`, installed HandsOnTest
app, offline dependency directories, real-weight evidence, uncommitted wire
fix, and this handoff. After cleanup, `C:` reported 332.76 GiB free.
