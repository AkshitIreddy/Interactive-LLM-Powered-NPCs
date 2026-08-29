# Security and privacy architecture

## Threat model

The application processes untrusted model output, provider responses, profile/model archives, game/window metadata, transcripts, potentially sensitive audio/frames, and data from user-selected executables. It runs beside games that may contain anti-cheat/protection. It must assume local files can be tampered with and network responses can be interrupted or stale.

Out of scope: bypassing anti-cheat, injecting into protected online games, copying actor voices without rights, or defending against a fully compromised administrator/kernel. The product must fail closed at these boundaries.

## Trust boundaries and controls

| Boundary | Principal controls |
| --- | --- |
| WebView → Rust | Narrow Tauri command allowlist, typed validation, response channels, no raw secrets/media, CSP and no arbitrary remote navigation. |
| Process IPC | Current-user SID pipe ACL, per-launch nonce, protocol/version handshake, message length/rate limits, sequence/deadline/cancellation validation. |
| Runtime → provider | Explicit egress policy by data category, TLS through supported client, credential resolved only inside runtime/provider adapter, redacted structured logs. |
| Runtime → workers | Typed bounded requests, immutable verified pack, Job Object, least handles, no profile code, no credentials. |
| Profiles/game boundary | The current built-in profiles are unsigned development data. A release must authenticate profile catalogs. Profiles remain data-only and cannot install or invoke mods, hooks, DLLs, script extenders, or executable adapters. Capture/overlay fails closed under online/anti-cheat ambiguity. |
| Model downloads | TUF metadata, HTTPS, declared size/hash, staging, safe extraction, self-test, atomic activation and rollback. |
| Memory/database | Per-user application data ACL, SQLite transactions, schema migrations, content scopes, secret-free diagnostic export. |

## Secrets

Provider credentials are written to Windows Credential Manager with a product/account-specific target name. Normal configuration stores only a non-secret credential reference. The runtime resolves a value for the minimum request scope and does not forward it to WebView, logs, media broker, inference workers, crash metadata or diagnostic exports.

Controls include:

- redaction by typed field before serialization, plus canary tests that scan all log/report outputs;
- no credentials in command lines or environment inherited by child workers;
- no fallback to plaintext when Credential Manager fails;
- explicit replace/delete UI and provider connectivity check;
- documentation requiring v1 plaintext keys to be revoked/rotated without reproducing them.

## Model/profile output safety

The v1 `temp.py` pattern is prohibited. LLM output is UTF-8 data subject to byte/token limits, spoken-text sanitization and independent `NpcEffectsV1` validation. Structured effects cannot request game actions or name modules, functions, commands, URLs or paths.

Profiles contain only schema-declared data. Community profiles cannot ship DLLs/scripts in the initial release. Archive import rejects absolute paths, `..`, alternate data streams, links/reparse points, device names, duplicate normalized paths, excessive entry count/size, decompression bombs, executables and unexpected file types.

Legacy pickle/Chroma indexes are rejected by extension and signature without deserialization.

## Update and model-pack security

- Release app and updater artifacts must be code-signed; the current local Debug installer is unsigned, and update signing keys are not present in the repository or build output.
- Public update-feed activation remains disabled until explicit user approval.
- Model catalog metadata uses TUF roles/thresholds to survive repository/signing-key compromise and prevent rollback/freeze/mix-and-match attacks.
- Each pack declares immutable source revision, all file hashes/sizes, runtime ABI, supported backends/hardware, resource envelope, license and attribution.
- Downloads land in unique staging, support bounded resume, are fully verified and self-tested, then activate through an atomic version pointer.
- Repair re-verifies every declared file; uninstall uses manifest-owned explicit paths and reference counts, never broad globs.

## Privacy model

Webcam perception is local, explicit and off by default. The application does not infer age, race or gender from faces. Media is ephemeral by default; diagnostic recording requires explicit scope, preview and retention controls.

Each feature declares possible egress categories: microphone audio, transcript, screenshots, webcam frames and game context. Onboarding and settings expose the selected route and consequences. No local-to-cloud or cross-cloud fallback occurs unless the user pre-authorized that exact route.

Remote telemetry is off by default. If introduced later, it must be opt-in, documented, inspectable and independent from essential operation.

## Required security tests

- fuzz every Protobuf payload and state transition, including oversized lengths, sequence gaps, old nonce/generation and deadline wrap;
- inject quotes/code/tool directives into model spoken/effects output and prove no execution or path access;
- feed malicious profile/model archives covering traversal, links, ADS/device names, duplicates, bombs and unexpected executables;
- prove legacy pickle files are never opened by an object deserializer;
- simulate TUF/signature/hash/size/rollback/freeze/mix-and-match failures;
- use credential canaries to scan UI events, logs, crash files, process command lines/environments and diagnostic exports;
- run Offline mode under deny-all networking and assert zero app egress attempts; separately verify that optional lip-sync packs never make provider calls;
- test unauthorized pipe connections from another user/session and stale same-user clients;
- test anti-cheat/shared-online detection and fail-closed capture/overlay refusal;
- run dependency, SBOM, license/provenance and known-vulnerability checks on every release candidate.
