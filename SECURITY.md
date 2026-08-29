# Security Policy

## Supported versions

Interactive LLM Powered NPCs 2.0 is pre-release. There is no supported public binary version or active update channel. Security fixes apply to the current 2.0 development branch; the legacy notebook/SadTalker prototype is unsupported and should not be run with real credentials or untrusted input.

## Reporting a vulnerability

Use GitHub private vulnerability reporting for this repository, if enabled, or a private channel listed on the maintainer's GitHub profile. Do not open a public issue containing exploit details, credentials, private logs, transcripts, or captures.

Include the affected commit/component, Windows version and execution mode, concise reproduction, impact, trust boundary, and sanitized logs. Do not access other people's data, test third-party services without permission, evade anti-cheat, or publish weaponized details before evaluation.

This volunteer pre-release project promises no response-time SLA. Reports will be acknowledged and triaged as capacity allows; disclosure timing will be coordinated for confirmed issues.

## Required security boundaries

- Current-user-restricted named pipes, protocol handshake, per-launch nonce, size limits, deadlines, and backpressure.
- Windows Credential Manager references; secrets do not enter WebView or model-worker prompts.
- Declarative, schema-validated game profiles with no arbitrary code.
- Separately versioned adapters, exact-build allowlists, and offline/protected-mode checks.
- Hash/signature-verified packs and updates, staged extraction, atomic activation, and rollback.
- Typed, allowlisted NPC actions validated by an adapter rather than executed from model text.
- Late-event rejection using session, turn, sequence, and cancellation generation.
- Content-light local logs and opt-in diagnostic export.
- Explicit provider routes with no silent local-to-cloud or cross-provider fallback.

Webcam perception is optional, visibly active, local, and off by default. Age, race, and gender inference are out of scope. The project does not attempt DRM, anti-cheat, process-protection, or online-service bypasses.

## Legacy warning

The 1.x tree contains examples that read plaintext keys, deserialize pickle data, and generate/execute Python from model output. Those paths are migration evidence only. Treat legacy notebooks, `voice.py` files, pickles, vector databases, and downloaded model trees as untrusted.

## Accidentally committed secrets

Revoke the secret with its provider immediately; deleting it later is insufficient. Report the exposure privately so history and downstream artifacts can be assessed. Never paste the value into an issue or diagnostic bundle.
