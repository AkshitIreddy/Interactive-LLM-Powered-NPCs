# Interactive NPCs 2.0 — successor handoff

Updated 2026-09-16 IST. This is a context handoff, not a prescribed work plan. The user wants the successor to assess the project and choose its own approach. The original request included reading `see me.md` completely; its dated implementation states and old executable paths are historical. This file records the newer state and the user's feedback.

## Latest user report: the test-game connection is still broken

The user's latest report is: **“connect to test app doesnt work even if i open it”**. They asked to save the entire context and investigation guidance for another AI agent, rather than continue implementation now.

The connection bug is OPEN. Earlier claims that launcher/status changes resolved it were based on component tests and package integrity checks, not successful installed-app connection. The user has already tried opening the game manually; that did not resolve the problem. There is no confirmed diagnosis or working fix for this report.

During this handoff turn, only source, documentation, Git state, specific metadata files, and named process presence were inspected. No app/game was launched, no capture/audio was started, no provider request was made, and no implementation was changed. The exact error text and exact button the user clicked are not available yet.

## Repository and delivered artifact identity

- Checkout: `C:\Users\akshi\Desktop\Code Palace\interactive llm\Interactive-LLM-Powered-NPCs`
- Parent workspace: `C:\Users\akshi\Desktop\Code Palace\interactive llm`
- Branch: `feat/2.0-overhaul`; confirmed on 2026-09-16.
- Implementation HEAD before this documentation-only handoff: `e482f1f7a9e58dacb1e96c24574502231019a248`; working tree was clean.
- App: `E:\temp\InteractiveNPCs\review-v21\interactive-npcs-control.exe`
- Test game: `E:\temp\InteractiveNPCs\review-v21\local-app-data\test-game\interactive-npcs-synthetic-target.exe`
- Package manifest: `E:\temp\InteractiveNPCs\review-v21\REVIEW-MANIFEST.json`
- Package instructions: `E:\temp\InteractiveNPCs\review-v21\REVIEW.md`
- Delivery record: `E:\temp\InteractiveNPCs\review-v21-delivery-20260915.md`
- Independent verification: `E:\temp\InteractiveNPCs\review-v21-independent-verification.json`
- App SHA-256: `b4f63f299f78aab4e0707bc407450c9f445870bc571a7a4bec67542ab0023be3`
- Test-game SHA-256: `6029eb869f6480a0d6fbdfa32f00c0b0912f717b6aaa7bdb2fe5872fc4c0fc06`
- Manifest SHA-256: `3a3d421e2d8622d9a2789182d0919b0abbe51f9eb04bdfbadfa73be0b160447f`
- Source candidate digest: `80bc6fecb2d8dec33582cdc572ba687d10e03d44c297435c663a4f7377bd9646`.

These identities describe the Sep 15 verified delivery; the user's exact running executable was not observed during the failure. The package is a Debug, installer-free, unsigned local review build: 814 manifest-bound payload files plus manifest, six executables. Its identifier is `io.github.akshitireddy.interactive-npcs.review`. No push, release, public update activation, or publication was performed. The v21 manifest binds its files, so in-place changes would invalidate that recorded package identity.

## Connection implementation and source observations

Intended flow: **Games → Included practice game → Launch & connect**. The standalone game is an original synthetic Eclipse Harbor/Mara Venn fixture, not Cyberpunk. Cyberpunk remains the real-game development focus; both identities are intentional.

Relevant files and responsibilities (line numbers describe this snapshot):

1. `apps/control/src/TestGameLauncher.tsx`: capability/status loading, launch action, retry, post-launch refresh, connection-loss polling, local errors.
2. `apps/control/src/ProductConsole.tsx`, around 1532–1648: `selectSyntheticTarget`, `verifySyntheticCaptureFor`, `connectReviewGame`. Separates preparing/selecting a target from checking advancing WGC frames.
3. `apps/control/src/tauriBridge.ts`, around 1889–1931: `prepareSyntheticReviewTarget`, status/capability and debug capture command wrappers. Tauri command registration is in `apps/control/src-tauri/src/lib.rs`.
4. `apps/control/src-tauri/src/commands.rs`, around 2930–3155: `select_synthetic_replay_capture_target`, `synthetic_review_target_status`, `prepare_synthetic_review_target`.
5. `apps/control/src-tauri/src/synthetic_review_target.rs`: co-located executable discovery, `REVIEW-FIXTURE-MANIFEST.json` validation, hash/size checks, child launch, explicit metadata argument, Windows Job assignment.
6. `apps/control/src-tauri/src/media_broker.rs`: `MediaBrokerLaunchConfig::from_application` (~2700), `debug_select_synthetic_replay_capture_target` (~3573), metadata reader/decoder (~3862), Windows PID/HWND identity validation and capture receipt.
7. `apps/control/src-tauri/src/game_targets.rs`: profile matching, selection, immutable selected target, revalidation, and `mark_debug_synthetic_broker_bound`.
8. `scripts/synthetic-game-replay/SyntheticGameReplay.cs`: options/default metadata path (~71), metadata publication/lifetime, source sequence and decoding.
9. `scripts/windows/prepare-review-test-game.ps1`, `verify-review-test-game.ps1`, `prepare-portable-review.ps1`, `verify-portable-review.ps1`: test-game assets, manifest and package assembly.

### Manual launch and review app metadata namespaces

Source inspection confirms the C# standalone game's default is:

`%APPDATA%\io.github.akshitireddy.interactive-npcs\debug-synthetic-replay-target.json`

The broker reads `config_directory\debug-synthetic-replay-target.json`. The review app uses the `.review` identifier/config namespace. The integrated launcher explicitly supplies `--metadata <broker path>` (and `--mute`), so it should avoid the standalone default mismatch when it successfully starts its own child.

This difference may be relevant to manual-open attach. Its role in the reported failure is unproven; the failing session's resolved configuration directory was not captured. Production and review preferences are intentionally separate. Current selection validates executable, process instance and HWND as well as reading metadata.

Read-only snapshot on Sep 16:

- Production-namespace metadata existed, last written Aug 30 03:30:23 IST; schema 1, `state=closed`, PID 9112, HWND 198360, 42 decoded frames.
- Review-namespace metadata existed, last written Sep 3 11:52:33 IST; schema 1, `state=playing`, PID 22472, HWND 1836818, 1891 decoded frames.
- No processes named `interactive-npcs-control`, `interactive-npcs-synthetic-target`, `npc-runtime`, or `npc-media-broker` were returned at that inspection. These files therefore do not prove a current connection despite the persisted `playing` value.
- No metadata was removed or changed. The actual failing-session metadata/process context is unavailable. JSON metadata uses snake_case, not camelCase.

### Intermediate errors are discarded

`prepare_synthetic_review_target` first attempts attach with `if let Ok(snapshot)`, discarding the first failure. It then launches another target and retries for ten seconds, with `Ok(_) | Err(_) => {}` discarding all intermediate reasons. The final timeout says the child failed to publish capturable metadata, even if metadata existed and a later profile/broker/admission step failed. The code kills only its launched child on timeout.

The operations within this path span metadata validation, process/HWND identity, fixture schema/hashes, game-profile selection, admission revocation, broker launch/authentication and WGC. The generic timeout does not identify which boundary failed. The launch helper also maps several OS/Job-assignment failures to a generic launch error.

### “Selected” and “capture verified” are separate states

`synthetic_review_target_status` says connected when the game selection revalidates. That alone does not establish advancing WGC frames. `connectReviewGame` prepares the target and then calls `verifySyntheticCaptureFor`; that function catches errors and sets a notice rather than propagating failure. A successful-looking launcher alongside failed capture is therefore a source-derived possibility, not a reproduced symptom. Notice visibility was an earlier UI defect and was changed during the last rework.

### Fixture contract and profile matching

The metadata decoder requires exact fixture kind/state, nonzero decoded frames, matching duplicate PID/HWND/executable fields, portrait hash, and schema-dependent source/motion fields. Moving schema 2 additionally binds sequence hash, dimensions, frame count/rate and content-frame data. C# produces this metadata and the Rust broker wrapper consumes it. Selection then binds the `eclipse-harbor` profile and revokes active loadout admission; failures after capture discovery can still be surfaced as launch timeouts.

An earlier agent attributed the issue to release/debug gating, but **v20 and v21 were Debug builds**. That explanation was withdrawn. The exact capabilities available in the user's failing session have not been observed.

## Product mission and architecture

A Windows app for talking to visible NPCs in single-player games: onboard, choose/detect game, configure cloud/local/hybrid models and stock voice, identify/select character, press-to-talk, receive spoken dialogue/subtitles and optional natural mouth motion. The game retains resource priority. Audio/subtitles should survive unavailable vision or lip-sync.

The user explicitly expects independent judgment: challenge previous agents' algorithms, architecture, docs, fixtures and completion claims; replace them where better solutions exist. Research changing providers/models using current official documentation and measured comparisons.

- `apps/control`: React/TypeScript + Tauri control plane.
- `apps/control/src-tauri`: native commands, configuration, broker/runtime supervision, resource/identity/loadout integration.
- `apps/runtime-host`: Rust turn supervisor, providers/streaming/cancellation/timing/memory.
- `native/media-broker`: Windows capture/audio/presentation boundary.
- `native/mouth-worker`: local mouth residual, source-pixel/observed oral-reference composition.
- `native/subtitle-renderer`: subtitle presentation.
- `crates`: typed contracts, credentials, providers, memory, profiles/discovery, model manager and diagnostics.
- `profiles`, `catalog`, `workers`: data-only game/content packs, model metadata and optional local runtimes.

Architecture entrypoints: `docs/architecture/overview.md`, `data-flow.md`, `process-model.md`, ADRs; broader requirement tracking: `docs/product-rework/original-brief-acceptance.md`, `original-brief-gap-map.json`, `docs/requirements`, `IMPLEMENTATION_STATUS.md`. These mix historical claims and implementation records rather than providing a uniformly current acceptance report.

V1 baseline: `main` at `503ef3b64a921b6a11efa9e3e0432a0c3de3b619`; `v1.0.0` at `9996575e69cf40d719e326adfea108476d80467b`. Read-only history is available through `git show`; analyses are in `docs/legacy/*`, particularly `v1-pipeline.md`, `feature-disposition.md`, `single-monitor-root-cause.md`, plus `docs/product-rework/v1-behavior-comparison.md`. V1 included known/background character recognition, per-character voices/lore/memory, RAG and PTT. It also contained unsafe generated-Python execution, credentials and pickle/Chroma caches; previous audits used source inspection rather than executing those artifacts. The user's requested working branch is the 2.0 branch.

## User preferences and feedback

The preferences below record the user's requests and existing operating constraints. They are distinct from the previous agent's implementation choices and unproven hypotheses above.

### UI and UX

- Premium, original, alive, video-game-like cyberpunk interface. Not a generic dashboard, cheap mockup, text-heavy architecture presentation or oversized landing page.
- The **whole app** must use space intelligently. Reorganize interaction hierarchy; do not merely shrink fonts/padding, clip overflow or make text harder to read. Plenty of options should remain understandable at a glance through progressive disclosure.
- Theme actual dropdown popups as well as closed controls and scrollbars. Keep primary actions visible; long catalogs/biographies/details can scroll internally or use focused dialogs. Avoid unnecessary main-page scroll at normal desktop sizes.
- No prominent complex Diagnostics tab. Keep useful real diagnostics contextual/advanced; remove simulated metrics and invented sessions/quests.
- Call the tab **Games**, not Night City. Cyberpunk is the current supported-content focus, but the product is intended to expand.
- The initial skeleton was conceptually wrong for cognition/sight/etc. Replaced with an original generated evil AI head: intimidating, angular, like the general idea of Ultron but not a copy. Connect roles to meaningful anatomical areas.
- Use the user's Cyberpunk screenshots as inspiration, not copied UI art. References were moved to `E:\temp\InteractiveNPCs\design-references\cyberpunk-20260908`.
- Be clear and concise about failures and next actions. The user dislikes preachy, repeated cloud/privacy warnings and explicitly approved AssemblyAI/cloud testing. Do not put implementation/compliance prose in ordinary product flows.
- Preconfigure only the user's private review setup; do not ship their credentials or make all users inherit that private setup.
- Make the included game easy to find and launch; users should not need a terminal or manual file edits.

### Lip-sync and content

- User repeatedly rejected cheap lip-sync: smoothing, natural anatomy and temporal consistency matter more than passing geometry metrics. Later said it looked nicer/better and asked to integrate loose ends; that was not blanket production qualification.
- Preserve the game's existing breathing, pose, blinking, lighting and camera motion. User said blink generation can be skipped because the game already handles it; mouth motion is our responsibility.
- Test multiple characters. Favor frontal/near-frontal idle characters with a visible mouth, rather than talking or extreme side-view footage. Pepe talking clip was disqualified. Handling incidental in-game speech is desirable later, not the current priority; suppressing game voice can wait.
- User-owned Cyberpunk video: https://www.youtube.com/watch?v=Uc3OXiFjsSg . User clarified it is Cyberpunk only, audio can be removed, and their original experiment had no lip-sync. Inspect chosen segments rather than assuming every segment is suitable.
- Character reference images can be found online for private preparation. Packs should eventually be modular, optional and downloadable, with world lore, backstories, voice/provider/model suggestions and user-overridable settings. No release is authorized yet.
- Don't require manually authored packs for every NPC in a large world. Develop a sensible unknown-NPC fallback, stable identities and editable background personas.
- Bios must be substantive and sourced, with speaking style and dialogue examples feeding actual prompts, not thin decorative text. Focus on Cyberpunk before claiming other games are ready.
- User wants videos showing quality and real latency numbers. Offline footage/component timings must not be presented as live-game or end-to-end proof.

### Providers and latency

- User questioned ~30-second hosted TTS latency as potentially our serialization/buffering mistake. Investigate streaming and pipeline overlap, first playable PCM versus full synthesis, rather than blaming the provider.
- A response beginning in roughly **5–8 seconds** is acceptable and a major improvement over v1's minutes. This is conversational onset after the question, not a permissible per-frame mouth-render time.
- API-first for everything except efficient local lip-sync is acceptable; keep optional local models for stronger PCs. Don't force heavy local stacks on ordinary users.
- Prefer generous/free options and offer choice. User named Groq, Mistral and Gemini as favored testing options; use OpenRouter and Cloudflare more sparingly. Current availability/quotas need live verification.
- User supplied Deepgram, Inworld, Cartesia, ElevenLabs (including a second key), and several LLM provider keys; ask for additional credentials only if actually necessary. Secrets live outside the repo in `C:\Users\akshi\Desktop\Code Palace\Commonly used Keys.txt`; never print or put values in docs, logs, bundles or commits.

### Workflow, display, storage

- **No flashing red/blue smoke test, visible helper/terminal, game/capture/GUI/audio test.** User raised a serious epilepsy concern. Hidden shell/CREATE_NO_WINDOW does not hide a GUI window created by a child. Do not run aggregate native CTest from an unaudited/stale cache. Use audited windowless checks and headless rendered artifacts. Their opening a game manually does not authorize us to launch flashing tests.
- A Transparency App dims their screen with an overlay, normally disabled when gaming. Desktop luminance can be misleading; distinguish exact-window capture from external overlays.
- On pause/save, stop owned work and write exact durable state; do not continue implementation behind the pause. Work autonomously on authorized tasks without repetitive permission requests. Keep focused local commits; no push/release/publish/tag/updater activation.
- GPU model work: read `C:\Users\akshi\Desktop\Code Palace\gpu use.txt`, coordinate ownership, restore the lock on every exit. Other projects/agents may be active. Do not infer GPU availability from a previous day's message.
- Large generated material belongs under `E:\temp\InteractiveNPCs`. Shared Cargo target: `E:\temp\InteractiveNPCs\cargo-target`; avoid duplicate agent/task/nested caches. Dev/test debug symbols and incremental compilation were disabled; tracked prevention fixes matter because `.cargo/config.toml` is locally ignored.
- User authorized deletion of genuinely unused/reproducible material. About 205 GiB of duplicate build artifacts was reclaimed earlier. Preserve source, current packages, credentials, selected models and unique evidence; verify exact paths/reparse points before Windows recursive operations.
- User revoked the old Windows deletion procedure and quarantine-first habit. `E:\uesless` was only a fallback if deletion was blocked, not a default destination. Do not reinstall that policy or remove unrelated skills.
- Keep the Windows Temp directory itself. Prior cleanup removed stale contents while skipping recent/locked/in-use/reparse entries; don't indiscriminately delete another project's active temp files.

## Latest implementation: completed work and its limits

### Whole-app UI

`b2a0bfd` reorganized Channel/Games/Loadout/Settings. Primary desktop actions fit at 1280×720 and 1440×900; long secondary content has accessible scrolling/dialogs. Settings uses one outer section list with horizontal categories and readable grouped fields. Games has a roster/inspector and directly visible practice-game launcher. Channel keeps composer and connection state visible.

Loadout has six roles (Cognition, Memory, Hearing, Sight, Voice, Mouth sync), provider/model editing, Global/Game/Character inheritance, persistent activation footer and focused Manage/Advanced dialogs. React Aria provides focus trapping, Escape and restoration. Local models uses Tracking/PC budget/Downloads/Benchmark/Advanced packs sections; microphone device and PTT panels sit alongside one another.

Main UI files: `ProductConsole.tsx`, `ProductWorkspaces.tsx`, `ProductPreferencesWorkspace.tsx`, `ProviderLoadoutEditor.tsx`, `CyberwareAnatomy.tsx`, `TestGameLauncher.tsx`, `workspace-layout.css`, `loadout.css`, `secondary-loadout.css`, `themed-controls.css`. Generated head asset: `apps/control/public/art/neural-interface-v2.png`.

Durable notes: `docs/product-rework/neural-workshop-2026-09-14.md`, `whole-app-layout-2026-09-15.md`. Evidence root: `E:\temp\InteractiveNPCs\ui-refinement-20260914` (fit, accessibility, secondary-loadout, workspaces, integrated). Normal populated layouts, forced colors and 200%-equivalent narrow state were inspected headlessly. Layout fixtures are not native connection evidence.

Final frozen-source frontend suite: **180/180 passed**, TypeScript passed. JSON: `E:\temp\InteractiveNPCs\ui-refinement-20260914\final-frontend-tests.json`. Defects corrected during review included clipped status text, truncated character names, inaccessible Local-model content, colliding settings controls, lost capture errors and errors hidden behind the Practice dialog.

Relevant tests: `TestGameLauncher.test.tsx`, `tauriBridge.test.ts`, `NativeSimulationEvidence.test.tsx`, `Honesty.test.tsx`, provider/loadout/workspace tests. Existing mocked successes did not catch the user's actual connection failure.

### Cyberpunk corpus and mouth packs

Corpus v1.2.0 contains **36 named characters plus one `night-city-resident`**, not all NPCs. Each named biography is roughly 169–205 words, two paragraphs, with role/relationships/motivation, personality, speaking style, voice intent, original illustrative dialogue and bounded knowledge. Prompt integration was tested.

Source: `scripts/content-packs/cyberpunk-character-corpus.mjs`; runtime: `profiles/games/cyberpunk-2077/profile.json`; research: `docs/research/cyberpunk-character-corpus-2026-09-14.md`. The v21 runtime profile and importable pack match all 37 ordered IDs. At packaging time the owner review namespace had no stale active Cyberpunk override; recheck before assuming that's still true.

Seventeen prepared private packs: Jackie Welles, Johnny Silverhand, Judy Alvarez, Panam Palmer, Viktor Vector, Misty Olszewski, Claire Russell, Rogue Amendiares, Kerry Eurodyne, Evelyn Parker, Alt Cunningham, Song So Mi, Solomon Reed, Rosalind Myers, Hanako Arasaka, T-Bug and Sebastian Ibarra.

All are schema-4 character-bound receipts, **disabled**, `naturalQualityQualified=false`, `ordinaryTargetsEnabled=false`. They are private component-review preparations, not certified live-game lip-sync. Source/provenance/boards/receipts are under `E:\temp\InteractiveNPCs\cyberpunk-character-mouth-packs-20260914`. Core packs use their documented per-character folders; seven expanded packs use `<id>\private-review-v2\pack-v1`, three (Hanako/T-Bug/Sebastian) use `<id>\review\pack-v1`. Inventory paths are in the delivery report/manifest.

Goro, River, Yorinobu, Dexter, Mama Welles and Regina attempts lacked accepted resolved oral-opening evidence and have no ready receipt. Generated identity-preserving mouth references have not established natural moving-game quality. Unknown characters have a conservative source-pixel fallback (`sealed_click_source_only=true`); no identity-specific oral texture means lower fidelity, not equivalent prepared-pack quality.

Moving Johnny proof: 28/42 frames warped, 14 smoke-occluded bypassed, 7.44 ms p95 component performance. Mouth was only about 20–30 pixels in dark footage, insufficient for honest natural-quality approval. Old v53 was promising source-preserving evidence; v66 was rejected for artificial dark holes/poor anatomy despite passing tests. OpenSeeFace's 66-point topology differs from dlib/iBUG68; exact historical indexing and composition findings are recorded in `see me.md` and retained research.

### Provider configuration and real latency boundaries

Sep 15 presence-only snapshot: `E:\temp\InteractiveNPCs\review-v21-provider-presence.json`. Private selected Cyberpunk loadout was `cyberpunk-private-fast-groq`: Groq `qwen/qwen3.6-27b`, AssemblyAI `u3-rt-pro`, Cartesia `sonic-3.6`, SQLite FTS5, vision/lip-sync disabled. This is a saved configuration snapshot, not a new live-provider benchmark or guarantee those models remain available.

Credentials were present for Gemini, Groq, Mistral, OpenRouter, Cohere, NVIDIA NIM, Deepgram, AssemblyAI, ElevenLabs, Cartesia and Inworld. OpenAI/Anthropic absent; Cloudflare had no current adapter. Owner settings/vault remain outside the package and were left intact.

`RuntimeTurnLatencyAssessmentV1` / checkpoint `467d451` measures finalized transcript to first decoded PCM, with 5000 ms target / 8000 ms ceiling. It excludes microphone/VAD/STT onset, physical speaker callback and visual/game-load evidence, so its numbers do not establish full question-to-audible-response latency.

### Packaging and catalog

`b10af8c` fixed a stale distribution-profile digest; `e482f1f` added the importable corpus and manifest/verifier binding. Staging and independent final-destination verification passed, including identity, file hashes, PE subsystem, strict source/license/provenance, test-game files, 17 receipts and private catalog crypto. **No GUI/game/capture/audio was launched.** Therefore the verifier's green status does not close the newly reported connection defect.

Private catalog root: `E:\temp\InteractiveNPCs\private-review-catalog-yunet-20260914-r5`. Catalog version `202609141801` expires **2026-09-21 18:01:46 UTC**; underlying envelope expires Oct 7 06:00:01 UTC. Expiration can make the same previously valid configuration fail on a later date. Existing refresh tooling used two ephemeral signers and did not change production trust or persist private signing material.

## Existing verification tools and unresolved areas

Frontend commands used from the repo root:

```powershell
node apps/control/node_modules/typescript/bin/tsc -b apps/control --pretty false
node apps/control/node_modules/vitest/vitest.mjs run --root apps/control
```

Headless layout harnesses (expect preview at port 1426; previous owned preview was stopped): `scripts/neural-workshop-fit-qa.cjs`, `secondary-loadout-fit-qa.cjs`, `workspace-layout-fit-qa.cjs`, `workspace-layout-populated-qa.cjs`, `cyberpunk-ui-review.cjs`. Node 20.20.2 / pnpm 10.28.2 were used by packaging. Some native tests create windows, which conflicts with the user's display-safety restriction.

Unresolved areas recorded at handoff, without prescribing their order or solution:

- User-observed test-game connection failure, including failure after manual launch.
- Complete installed-app conversation evidence with configured providers, identity, subtitles and end-to-end latency.
- Natural moving-character mouth quality, smoothing, identity/occlusion behavior, provider cue synchronization and unknown-NPC fidelity.
- Broad game-load/resource admission, cross-process presentation/HDR, real Cyberpunk behavior and clean install/repair/recovery acceptance. Historical component checks cover parts of these, not the complete product.

Other helpful commits: `9b468b8` launcher status/refresh, `259e0cb` availability/error propagation, `62b18ed` themed controls, `47121f4` selector alignment, `07fe5be` compact loadout, `3eaac5a` dialog-aware credential tests, `12081ee` corpus expansion, `8abd21a` mouth-pack pipeline, `78066d9` catalog rotation, `e1d1989` cache prevention.

The user explicitly wants the successor to use this context to reach its own conclusions. No algorithm, diagnosis, architecture or work sequence in earlier agent notes is an obligation to preserve that approach.
