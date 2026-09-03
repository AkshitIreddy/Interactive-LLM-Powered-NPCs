# Paused handoff — 2026-08-30

Checkpoint time: `2026-08-30T10:04:17Z`  
Resume only after the user asks to continue.

## Pause integrity

- Branch: `feat/2.0-overhaul`
- Base commit: `7acf5026b7f9b46170b49bd8f7567a5abbacd873`
- Worktree: intentionally dirty; 410 porcelain entries at pause
- Porcelain-list SHA-256 before adding this handoff: `48e4a502064bd022965d8b1e288d4c177115e2339283fc8de3f89182aaa0dc46`
- `git diff --check`: passed at pause
- No commit, push, tag, package, installation, release, or updater action was performed.
- Every active agent was interrupted. Task-owned Cargo/privacy-hook and Vite/esbuild process trees were terminated. A final WSL and Windows process inventory found no remaining repository command, `npc-*` process, qualification process, or development server.
- No GUI was launched during the final pause/checkpoint sequence. Transparency App and unrelated user windows were not moved, focused, closed, or reconfigured.

## Coordination state

- `C:\Users\akshi\Desktop\Code Palace\gpu use.txt` is still exactly `yes`.
- Its unchanged timestamp is `2026-08-30 07:48:39.894357900 +0000`.
- This task did not acquire, reset, or release that external lock. Do not run Qwen, Kokoro, YuNet/SFace, OpenSeeFace, or any other AI inference until the file is `no` or the user confirms the existing `yes` is stale. CPU model inference also requires the lock.
- Hosted-provider bounded tests had already completed and their remote sessions/transcripts were cleaned. Do not repeat provider calls merely to reconstruct state.

## Frozen and verified before pause

- WGC lifecycle, minimize/restore, device recreation, exact-HWND capture, display simulation matrix, and Transparency App independence.
- Native media broker commands 24–30, real WASAPI playback/input, endpoint rollback equality, and arm-then-newer-F8 PTT semantics.
- Opaque one-time AssemblyAI STT receipt: strict native scope, cancellation, replay/expiry rejection, runtime receipt preservation, and frontend accepted-receipt proof.
- Character DB, game-scoped memory, discovery, 20 authored profiles plus Eclipse Harbor, migrations, backup/restore/erase, and prompt provenance.
- Commands 21/22 private/original identity reference import boundary and native actor-lock DTO/revocation contract. Automatic identity remains unqualified.
- Subtitle presenter command 23 and native DPI/HDR/color/presentation receipts.
- Subtitle preference core in `crates/subtitle-engine/src/preferences.rs`: 25/25 tests, global/game/character inheritance, safe area/text scale/backplate/opacity, font/license disclosure, migration and fail-closed persistence.
- Hosted provider matrix and policy: Cohere, ElevenLabs, AssemblyAI, and NVIDIA private-evaluation routes. NVIDIA base production use remains blocked by the provider-wide Trial Terms gate.
- Provider-loadout regression suite reached 24/24 after the NVIDIA fixture correction.
- Product benchmark evidence accepts only product-runtime receipts; historical/synthetic observations cannot promote results.
- BGE local embedding and OpenSeeFace signal-pack current-device qualifications. OpenSeeFace is a landmark signal, not a lip-sync model.
- Resource governor and optional-pack install/repair/remove/race/cancellation contracts.
- Diagnostics/privacy schemas, redaction, 15-row matrix, packaged telemetry inventory, and the installed WFP deny-all collector/validator source.
- Visual coordinator: exact actor-lock/current-frame/command29 bindings, stale/expiry/cancel fail-open, runtime start/stop wiring, and final focused/all-target green report. It remains intentionally dormant because no admitted actor authority is active.
- Installed-app evidence harness in `scripts/windows/installed-app-evidence/`: 35/35 no-GUI tests. It has not captured a GUI and must not run without an exact frozen package/source authorization token.
- Source hygiene, root dispatch, Corepack/toolchain/WebView2/package-contract, acceptance-consistency, and security-tool pre-freeze gates were green before the newest interrupted integrations.

## Interrupted work — treat as unverified

The following files were being edited when the pause arrived. Their source is saved, but a passing earlier build does not certify the current bytes.

1. **Effective configuration aggregation**
   - `apps/control/src-tauri/src/effective_configuration.rs` exists.
   - `commands.rs`/`lib.rs` registrations were added.
   - Four focused tests were written, but the final compile/test pass was interrupted.

2. **Subtitle preference product wiring**
   - AppState and Tauri read/save/reset registrations appear in `commands.rs` and `lib.rs`.
   - Frontend wiring was in progress. Recheck `ProductConsole.tsx`, `ProductWorkspaces.tsx`, bridge DTOs, revision conflicts, reset confirmation, and renderer consumption.

3. **Frontend final polish**
   - Accepted STT completion proof was focused-green 43/43.
   - Optional-pack cancel, searchable bundled support guides (`supportGuides.ts`), diagnostics `fileName`, subtitle settings, and effective-configuration inspector were being integrated afterward.
   - The last fully broad run before those additions was 107/107 plus build; rerun typecheck, all Vitest, build, formatting, command reachability, and headless visual checks.

4. **Packaged privacy proof**
   - `packaged_privacy_receipt.rs`, `capture-installed-privacy-proof.ps1`, installer-smoke hook, WFP event capture, schemas, and validators are present.
   - The contradictory Debug guard was removed just before pause.
   - Final focused rerun was interrupted. `offline_mode` may emit only from native route truth; `local_lip_sync` must emit no receipt until a complete admitted local visual route exists.

5. **Runtime product timing**
   - `apps/runtime-host/src/runtime_timing.rs` is a draft/source addition.
   - It must remain runtime-owned and versioned; runtime-host must not depend on the benchmark crate.
   - Finish exact QPC-domain LLM request/first-token/structured terminal, TTS request/first decoded PCM/final PCM, cancellation, route/voice/egress binding, sidecar serialization, Control conversion, broker/game/telemetry join, and missing/unordered/mixed-clock tests.

6. **Qualified identity runtime authority**
   - `LocalResourceManager::admitted_identity_launch` and closed-world Python/OpenCV/NumPy runtime authority code were added with tests near the end.
   - The final focused/all-target verification was interrupted.
   - AppState still does not own a production `IdentityControlBridge`/worker service. Never expose private paths, pixels, model handles, or activation authority to the WebView.

7. **Identity qualification harness**
   - `workers/local-identity/qualification_harness.py`, examples, plan updates, and tests were being written.
   - No weights were downloaded or run.
   - Recheck completeness, hidden process cleanup, exact parent-grant/GPU-lock requirements, rights-cleared open-set corpus, lifecycle sample counts, and unsigned-evidence behavior.

8. **Manual addressed-actor picker**
   - The command-31 contract audit had only started. A source search at pause found no command-31/manual-picker implementation.
   - Resume as an additive native broker path: one real pointer click in a non-activating overlay, exact game HWND/current WGC frame binding, no pixels/coordinates from WebView, click-inside-current-detected-ROI rule, cancellation/resize/DPI/device/target-loss failure, and no highest-score or memory-based actor choice.
   - Extend actor-lock provenance so `sealed_native_click + admitted visual pack` is distinct from qualified automatic identity. Do not reuse identity-pack fields.

9. **Visual/identity activation**
   - Ordinary runtime visual calls are reachable but currently yield a typed unavailable/fail-open receipt and never call the presenter.
   - Finish either the sealed manual-click authority or a fully qualified automatic identity authority before activation. Audio and subtitles must remain independent.

## Known deliberate gates

- Catalog/legal reconciliation is intentionally stale while Kokoro, Qwen, and identity manifests are unfrozen. Strict license inventory currently rejects the changed `catalog/v1/catalog.json`; do not refresh hashes until all model inputs are final.
- No final app, installer, installed distribution, privacy proof, rendered installed-app evidence, or regenerated `local-review.md` exists yet. Do not hand the user an older Debug executable.
- Physical single-monitor, active HDR, negative-origin, 150/200% Windows DPI, real exclusive fullscreen, and physical operator F8 remain unmeasured; simulated/WebView-effective profiles must stay labeled as such.
- NVIDIA NIM is an optional one-account private-evaluation route, not unlimited production entitlement and not a generic captured-game lip-sync service.

## Resume order

1. Confirm the user asked to resume; verify `gpu use.txt` without changing it.
2. Confirm there are no leftover task processes and inspect `git status`/recent files. Remove only generated `__pycache__`/test caches after verifying they are not source.
3. Run formatting plus narrow compile/tests for each interrupted lane before changing behavior. Do not start with a package build.
4. Finish effective configuration, subtitle wiring, help/cancel UI, packaged privacy receipt, and runtime timing receipt; rerun the full Control/runtime/frontend suites.
5. Finish and adversarially review command 31/manual actor authority, identity runtime authority, and visual activation. Keep the capability unavailable until exact admission exists.
6. When the GPU lock is legitimately available, root alone sets it to `yes`; run Kokoro, Qwen Vulkan/CUDA, and YuNet/SFace qualifications **serially**, cleaning processes/VRAM between each; root restores `no` after every lane.
7. Freeze manifests, issue the final locally trusted review catalog/envelopes, then rerun legal/security/reproducibility and the entire source matrix.
8. Only then prepare all required sidecars, create the isolated `.review` package, run two-install reconciliation, real WFP privacy capture, synthetic-game/provider/audio/visual benchmarks, and installed visual QA.
9. Regenerate the 52-row acceptance ledger and `local-review.md` from exact artifact hashes. Give the user the app and synthetic-game paths only at that point. Do not push or release.

