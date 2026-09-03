# Local diagnostics contract

`interactive-npcs-diagnostics` is the shared, dependency-light contract for
truthful product health checks and user-initiated support exports. It does not
contain an HTTP client, remote telemetry destination, automatic uploader, raw
crash dump collector, or an API for attaching prompts, transcripts, audio,
frames, screenshots, or arbitrary files.

## Integration flow

1. Create one `CrashMarkerStore` in the app-private diagnostics directory and
   call `begin_session`. Update only the typed `RecoveryPhase`; call
   `mark_clean_exit` after an orderly shutdown.
2. Create one `LocalEventLog` with a unique non-secret session ID. Emit
   `DiagnosticEvent` values with explicit `ObservationProvenance` and
   `TimingEvidence`. The store validates, redacts, bounds, and rotates JSONL
   before it reaches disk.
3. Build the exact 15-row product matrix with `DiagnosticMatrixBuilder`:
   provider credential presence (never value/hash/length), microphone, speaker,
   STT, TTS, LLM, model-pack integrity, model admission, capture, GPU, VRAM,
   game target, overlay, latency, and OS permissions. A measured result requires
   a UTC observation time; any duration requires measured timing evidence.
   `fill_unmeasured` creates explicit skipped rows with remediation and never a
   readiness claim.
4. Build a user-previewed export with `DiagnosticsExportBuilder`. The builder
   recursively redacts the complete typed document, enforces count and 2 MiB
   size ceilings, embeds the local-only privacy declaration, and returns a
   SHA-256 preview. The desktop shell, not this crate, owns the explicit save
   dialog.

The bundled machine-readable schema is exposed as
`DIAGNOSTICS_EXPORT_SCHEMA_JSON` and lives at
`schemas/diagnostics-export-v2.schema.json`.

## Suggested actions

Suggested actions are declarative and closed over `SuggestedActionKind`. There
is no command, executable, argument, or remote URL action. The consuming app
maps the action kind and bounded target ID to its own trusted UI operation; it
must never execute the display label.

## Storage bounds

`LocalEventLogConfig::conservative` keeps one 512 KiB active file and three
rotated files (2 MiB maximum managed storage), with a 16 KiB maximum serialized
event. Custom values are capped at 16 MiB per file, ten rotations, and 64 KiB
per event. Oversized/corrupt managed files are discarded and reported in the
next `EventWriteReceipt`; corrupt records are counted without echoing their
contents.

Crash markers are fixed-name, typed JSON capped at 16 KiB. They contain only
session/version/timestamp/phase/counter metadata and never a stack trace,
minidump, provider payload, or user content.

`LocalEventLogConfig::verbosity` supports essential, standard, and verbose
local detail. Essential still records every warning and error. Verbosity cannot
enable remote telemetry or content-bearing fields. Use
`DiagnosticEvent::with_turn_id` to add a bounded, redacted turn correlation ID
without placing dialogue in the event.

## Canary and packaged privacy proof

`SecretCanarySuite` scans source, IPC, structured-log, report, and export bytes
in memory and returns only finding kinds/counts. It never returns matching
material, context, or fingerprints. Use only the synthetic suite; never seed it
from Credential Manager or environment variables.

`PrivacyProofBundle` is the release acceptance contract for remote-telemetry
absence and two OS deny-all scenarios: Offline mode and local lip-sync. A
passing bundle must bind the same release candidate, package manifest, package,
installed-distribution manifest, and executable SHA-256 identities. Every proof must be a measured packaged
executable observation with a bounded run ID and strict UTC timestamp. Fixtures
and internal harnesses cannot validate as a pass. The machine-readable schema
is `schemas/privacy-proof-v1.schema.json` and is exposed as
`PRIVACY_PROOF_SCHEMA_JSON`.

The bundle also binds `captureEvidenceSha256`, the retained Windows Filtering
Platform/process observation sidecar produced only against an installed
candidate. The independent post-package validator checks that sidecar's exact
artifact identities and installed-manifest process hashes; the compact Rust DTO
does not embed OS event-log evidence directly.

The crate provides the contract, not the evidence. Do not mark R32/R33 passed
until the final packaged candidate has actually run under OS deny-all controls
and produced a validating bundle.
