# Diagnostics and safe reports

Diagnostics should answer “what failed, where, and what safe fallback is active?” without collecting conversation content by default.

## Session diagnostics

The Response Spine shows measured stage state, latency, provider/model route, retry/quarantine state, capture freshness, audio health, active character source, memory mode, and any degradation reason.

## Export contents

A diagnostic bundle may include build/commit, OS/hardware/driver/display, sanitized configuration, profile/build/capability status, optional lip-sync pack manifests/hashes/attestation state, provider adapter status without keys, structured span timings, typed errors, worker exits, and a user-selected time range.

It must exclude credentials, full prompts/responses, raw transcripts/audio/video/webcam frames, memory contents, proprietary game assets, home-directory details, and provider payloads unless the user explicitly previews and opts into a specific attachment.

## Before sharing

1. Reproduce once in deterministic simulation where possible.
2. Export the narrowest time range.
3. Open the report preview and redact names, paths, IDs, dialogue, and captures.
4. Attach exact reproduction and expected/actual behavior.
5. Use private security reporting if exploitation or a secret is involved.

## Health states

- **Ready:** verified and within configured budget.
- **Degraded:** fallback active; conversation can continue.
- **Retrying:** bounded recovery for an in-flight failure.
- **Quarantined:** repeated worker/feature failure; disabled for this session.
- **Blocked:** policy/safety/build ambiguity prevents the operation.

Unknown errors must retain a correlation ID and actionable component, not expose a stack trace to the user.
