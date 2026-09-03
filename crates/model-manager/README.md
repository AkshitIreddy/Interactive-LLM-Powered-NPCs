# Model Manager security boundary

`model-manager` owns signed model-pack metadata, download integrity, safe staging,
self-test authorization, atomic activation, rollback, repair, and removal.

## Attested self-tests

Production activation cannot be authorized by a caller-supplied boolean or output digest.
The manager issues an OS-random, short-lived `SelfTestChallengeV1` bound to:

- pack ID and immutable revision;
- manifest digest and signed-catalog payload digest/version;
- exact staging transaction and installed-content tree digest;
- runtime ABI and approved backend;
- test-suite ID and immutable suite revision;
- nonce, issue time, and deadline.

A trusted runtime implements `SelfTestAttestationVerifier` and authenticates the returned
`SelfTestAttestationV1`. After extraction and indexing, the filesystem backend atomically renames
the verified tree into an immutable installed-inactive version without creating an active pointer.
The manager validates every self-test binding again, durably consumes a replay key, and creates a
crate-private `ActivationAuthorization`. Immediately before activating that exact version, the
filesystem backend re-hashes the installed tree and requires the exact attested binding; only then
may it atomically update the protected active pointer. Mismatch, expiry, replay, unknown runner,
invalid proof, or post-test modification fails closed.

`MockAttestedExecutorVerifier` is available only to crate tests or the explicit
`test-support` feature. It is deterministic and intentionally not cryptographically secure.
This crate does **not** provide or claim a production model runner, runner key provisioning,
or runner sandbox. The trusted runtime must supply those components.

## Windows path safety

Windows file creation retains exclusive `CreateFileW` handles for the protected directory
and every ancestor below it while the target file is open. Reparse-point attributes and volume
identity are checked from live handles, and the post-open canonical target must remain beneath
the protected root. Unprepared download directories and any unsupported safe-open condition
fail closed. Atomic state replacement uses a protected parent handle and write-through rename.

Archive extraction uses the same protected creation primitive after a complete metadata
preflight. ZIP/TAR links, devices, traversal, alternate data streams, reserved device names,
case-fold collisions, expansion bombs, and file/directory collisions are rejected before any
member is written. An archive artifact may additionally declare one exact `strip_prefix` and a
closed-world `required_paths` allowlist. Every member must be beneath the prefix, only allowlisted
files are installed, every required file must be present exactly once, and license/third-party
notice files are governed like executable runtime files.

## Optional-local model qualification

The optional-local catalog contains review candidates, not trusted downloads. A candidate becomes
`QualifiedLocalModelV1` only when an installable entry from the threshold-signed catalog matches its
capability, generic scope, upstream source/revision, Windows runtime/ABI, and reviewed license
metadata exactly. The signed manifest pins every artifact by size and SHA-256. Revoked entries,
unresolved license permissions, non-optional-local channels, or any mismatch fail closed.

OpenCV SFace remains visible as a private-evaluation research candidate, not a redistribution- or
commercially-qualified pack. The repository's Apache label is not treated as proof of rights for
the exact pretrained weights or their training data; qualification remains blocked until those
permissions are explicitly resolved and the candidate record is promoted. The project-owned
causal viseme mouth-warp baseline is model-free reference CPU code, not a GPU model.

The optionally downloaded OpenSeeFace MNV3 + LM1 pack is a separate experimental `Vision`
dependency of that mouth-warp baseline. It supplies current-frame landmarks/signals at no more
than 15 Hz on the CPU; it is not complete lip-sync, identity recognition, lip reading, or a
talking-head generator. Its strict v2 payload pins two model files, the OpenSeeFace license, and
the exact ONNX Runtime archive. Runtime extraction installs only the declared DLL, shared provider,
version/commit, license, and `ThirdPartyNotices.txt` inventory. Activation is bound to that exact
installed tree, the guarded replay self-test, one-thread CPU execution, queue depth one, and a
mandatory actor/track/frame/scene plus appearance/occlusion latch. Rejected, late, stale,
cancelled, or scene-mismatched work must leave the current frame untouched.

Until release threshold signatures exist, `npc.local-review-catalog/dev-v1` provides a separate
cryptographically testable review path using a published RFC 8032 test key. The corresponding
private test vector is public, so its receipt is labelled `LocalReviewDevOnly`; it has a different
schema and Rust result type from `TrustedCatalog`, and production catalog verification rejects it.

The exact UI/native-command handoff reserved for this pack is:

- `install_experimental_model_pack` with `pack_id`, immutable `revision`, and
  `explicit_user_confirmation: true`;
- `activate_experimental_model_pack` with `pack_id`, immutable `revision`, and the selected
  mouth-warp profile ID;
- `repair_model_pack` with `pack_id` and immutable `revision`;
- `remove_model_pack` with `pack_id` and `require_unreferenced: true`.

These names are a typed handoff contract in this crate; a native/UI bridge must implement them
before controls are exposed. They are not shell commands and this crate does not execute them.

Planning estimates are represented by a separate `PlanningResourceEstimateV1` type and must carry
`not_for_admission: true`. They can inform setup screens, but can never authorize activation or a
runtime loadout. Admission instead requires a threshold-signed `MeasuredResourceEnvelopePayloadV1`
bound to the exact manifest digest, device/runtime fingerprint, benchmark-suite revision, sample
count, validity interval, and monotonic report sequence.

`ResourceGovernorV1` evaluates the complete requested loadout against a fresh live snapshot. It
protects the larger of observed and configured game VRAM, desktop VRAM, a proportional/minimum
safety reserve, the OS VRAM budget, RAM availability, and configurable soft ceilings. An unmeasured
placement or wrong device fails closed. The residency tracker supports keep-warm, CPU-resident /
GPU-cold, and unload-TTL transitions; the scheduler prioritizes lipsync, speech, and dialogue while
cancelling superseded visual work and deferrable embedding/vision work.

## Frozen selected-loadout API

`SelectedLoadoutManagerV1<V, E>` is the native aggregate boundary. The WebView-safe
`SelectedLoadoutSelectionV1` contains only a bounded selection ID, exact catalog `PackRevision`s,
roles, preferred residency, and an explicitly labelled expected-idle scheduling target. It rejects
unknown fields. PID, clocks, persisted reserves, telemetry, catalog trust, signature verification,
and envelope storage cannot be deserialized through that DTO.

Native code constructs `NativeAdmissionContextV1` and calls `admit`. The manager then:

1. requires the telemetry snapshot to name the exact non-zero target PID;
2. resolves each immutable manifest from its owned current `TrustedCatalog`;
3. loads each signed envelope through its owned `SignedResourceEnvelopeSourceV1`;
4. verifies signature threshold, sequence, validity window, device, manifest digest, capability,
   sample count, and exact placement;
5. admits the complete loadout, including p99 RAM, resident plus transient VRAM, current desktop
   pressure, the greater of measured/configured game VRAM, game RAM growth reserve, and safety;
6. returns a Serialize-only `SelectedLoadoutDecisionV1` with typed status/reason, exact PID,
   selected roles, snapshot, optional non-deserializable receipt, measured residency decisions,
   and pressure cancellations.

`planner_snapshot`, `plan_maintenance`, `submit_work`, `advance_visual_clock`, `apply_pressure`, and
`pop_next_work` complete the control handoff. Residency compares the signed placement's measured
`p99_reload_millis` with expected idle before choosing keep-warm, CPU-resident/GPU-cold, or unload.
The bounded scheduler evicts lower-priority queued work, sheds deferrable work under pressure, and
drops lip-sync work whose generation/frame is stale. None of these paths downloads or runs a model.

## Canonical manifest and release catalog

`parse_and_normalize_model_pack_manifest` is the one runtime boundary for the
strict `npc.model-pack/v2` document and the conservative generic-v1 migration.
It preserves the existing install/lifecycle core without allowing role-specific
legacy schemas or planning estimates to bypass v2 trust and measurement gates.

`build_release_catalog_bundle_v1` produces the compatibility planner catalog
plus a separately threshold-signed v2 source inventory. The inventory binds raw
and canonical v2 digests, the normalized core digest, admission/license state,
and real qualified-envelope report digests. `verify_release_catalog_bundle_v1`
verifies both payloads and their exact relationship; an older reader may consume
the compatibility payload, but product initialization must verify the complete
bundle. No signing key or fabricated measurement is bundled by this crate.
The checked local-review bootstrap root is non-production metadata: it claims
no signer independence, requires rotation before release, and explicitly
disables promotion/publication. Its only purpose is deterministic local product
review of catalog selection and fail-closed measurement handling.

## Testing

Tests use local mock HTTP only. No model is executed, no live artifact is downloaded, and no
production runner identity is bundled.
