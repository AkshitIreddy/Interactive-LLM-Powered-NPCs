# Diagnostics and privacy integration contract

This document is the exact control/runtime handoff for R23, R24, R32, and R33.
It defines code integration; it does not claim the current machine or a packaged
release candidate has passed checks that have not run.

## Product matrix

The control shell must return every row in `PRODUCT_DIAGNOSTIC_MATRIX`:

| Check ID | Native source | Required remediation target |
| --- | --- | --- |
| `credentials.providers` | Credential-vault presence status only | `providers` |
| `audio.microphone` | Selected capture endpoint + permission probe | `microphone` |
| `audio.speaker` | Selected render endpoint + bounded playback probe | `speaker` |
| `provider.stt` | Selected STT route readiness/health | `stt` |
| `provider.tts` | Selected TTS route readiness/health | `tts` |
| `provider.llm` | Selected LLM route readiness/health | `llm` |
| `models.pack_integrity` | Signed manifest/hash/self-test state | `models` |
| `models.admission` | Whole-loadout measured admission | `models` |
| `media.capture` | Media broker capture freshness/backend | `capture` |
| `hardware.gpu` | Native adapter/driver telemetry | `hardware` |
| `hardware.vram` | Live dedicated-budget telemetry | `models` |
| `game.target` | Selected process/profile safety evidence | `game` |
| `media.overlay` | Broker overlay backend/permission | `overlay` |
| `runtime.latency` | Correlated measured stage timings | `performance` |
| `system.permissions` | Native mic/capture/storage checks | `privacy` |

Construct measured `CheckResult` rows first, call `fill_unmeasured`, then
`build`. This guarantees missing probes remain visible and actionable. Use
`ProviderCredentialPresence` for each allowlisted hosted provider; no WebView,
IPC, event, report, or export DTO may contain credential value, hash, length, or
fingerprint.

## Tauri and runtime wiring

- `diagnostics_v2_matrix(correlated_turn_id: Option<String>) -> DiagnosticMatrixResult`
  gathers native state only. WebView input cannot select provenance, status,
  provider/model evidence, timestamps, timing, or metrics.
- `diagnostics_v2_snapshot(max_events)` returns already-redacted stored events.
- `export_diagnostics_v2({ maxEvents, explicitUserConfirmation })` remains an
  explicit user action and reports `uploaded: false`.
- Persist `DiagnosticVerbosity` as an allowlisted enum. Apply it through
  `LocalEventLogConfig::verbosity`; warnings/errors must remain recordable at
  every setting.
- Runtime, provider, media broker, and worker events use one turn identifier via
  `DiagnosticEvent::with_turn_id`. Do not put transcript, prompt, audio, frame,
  path, or provider payload data in labels.
- Begin the crash marker before subsystem startup, update typed phases only,
  surface prior unclean recovery metadata, and remove the marker only after all
  task-owned services shut down cleanly.

## Canary and release evidence

Run synthetic canaries through source scanning, serialized IPC, local JSONL,
legacy report, v2 export, and crash metadata. `SecretCanarySuite` results contain
counts only. The repository scanner similarly prints only file/source/rule
metadata and must never print matching content.

Release acceptance requires one `PrivacyProofBundle` for the exact packaged
candidate. It must contain a measured remote-telemetry-absence proof and
measured OS deny-all proofs for both Offline mode and local lip-sync, all bound
to the same release-candidate ID and package-manifest/package/installed-manifest/executable SHA-256 values.
Fixture or test-harness results are never packaged proof.

After packaging, installation, and closed-world installed reconciliation, opt
the installer smoke into `-CaptureInstalledPrivacyProof`. Its pre-cleanup hook
invokes `scripts/capture-installed-privacy-proof.ps1` while the exact second-cycle
installed candidate still exists. The collector runs hidden, requires an
already-elevated host and pre-enabled Windows Filtering Platform failure audit,
applies temporary per-program outbound block rules to every reconciled installed
executable, and removes those rules in `finally`. Each scenario must return a
fresh candidate-process receipt bound to its PID, executable hash, run ID, and
scenario. A fixture, stale receipt, incomplete scenario, nonzero provider
request, or any external WFP attempt fails before proof creation. Loopback
activity is counted separately and retained in capture evidence.

The companion `measure_packaged_telemetry_inventory.py` hashes the frozen
Cargo/pnpm dependency inputs and every closed-world installed file before it
checks bounded binary surfaces for remote telemetry dependencies, upload entry
points, and telemetry destinations. It reports counts only and never reads
credential stores.

Finally run `scripts/validate-installed-privacy-proof.ps1` with the measured
proof, retained capture evidence, package manifest, installer,
installed-distribution manifest, installed control executable, and
release-candidate ID. The validator independently hashes every artifact and the
capture sidecar, binds every observed process hash to the installed manifest,
and rejects fixture provenance, test-harness enforcement, duplicate runs,
omitted scenarios, nonzero egress, or schema drift. No source-only or fixture
run can enter packaged acceptance.
