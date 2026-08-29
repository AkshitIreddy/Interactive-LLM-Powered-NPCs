# ADR-0009: OS credentials, TUF model packs and a non-injecting game boundary

Status: Accepted  
Date: 2026-08-28

## Context

Version 1 stores API keys in JSON and executes generated Python. Version 2 adds remote providers and downloadable models/profiles beside arbitrary games, making explicit trust/update boundaries mandatory.

## Decision

- Store provider secrets in Windows Credential Manager; configuration retains only opaque references.
- Sign application artifacts and keep public update activation disabled until approval.
- Protect the downloadable profile catalog and any future optional generic lip-sync pack catalog with TUF metadata; verify every file size/hash before safe extraction and atomic activation. Conversation models are not downloadable product packs.
- Keep every built-in and community profile data-only. Do not distribute, install, or invoke per-game mods, hooks, script extenders, DLLs, or executable adapters. Refuse capture/overlay whenever online/protected/anti-cheat status is ambiguous.
- Generate CycloneDX SBOM, third-party notices and machine-readable library/model/profile-content provenance for every release candidate.
- Reject executable model/profile output, legacy pickle/index state and archive traversal rather than trying to sanitize/execute it.

## Consequences

- Signing/TUF key custody and threshold/rotation procedures are operational release requirements, not repository secrets.
- Offline/local users can verify installed packs without contacting a provider.
- Repair/rollback/removal must be manifest-driven and reference-counted.
- Game coverage may degrade to audio/subtitles instead of taking a safety risk.

## Evidence

- Windows Credential Manager `CredWriteW`: <https://learn.microsoft.com/en-us/windows/win32/api/wincred/nf-wincred-credwritew>
- TUF specification: <https://theupdateframework.github.io/specification/latest/>
- CycloneDX specification: <https://cyclonedx.org/specification/overview/>
