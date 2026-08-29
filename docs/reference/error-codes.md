# Error categories

Machine-readable numeric/string codes are generated from the runtime contract once stable. This reference defines user-facing categories without inventing identifiers.

| Category | Meaning | Expected response |
| --- | --- | --- |
| Configuration | Missing/invalid route, model, device, profile, or permission | Open the named setting; simulation remains available. |
| Policy blocked | Online/protected/anti-cheat/build ambiguity | Do not retry around the policy; use a safe offline mode or audio-only allowed path. |
| Credential | Missing, rejected, expired, or insufficient provider credential | Test/replace the Credential Manager entry; never share it. |
| Provider | Network, quota, rate limit, unavailable model, invalid response | Retry only as advised; no unapproved provider switch. |
| Model pack | Catalog/signature/hash/size/license/ABI/self-test/storage failure | Preserve active pack; retry, repair, or remove staged content. |
| Audio | Device loss, capture/playback format, VAD/PTT, underrun | Select device or typed/subtitle fallback. |
| Capture/display | Window unavailable/protected, device loss, mapping/HDR failure | Recover capture or continue audio/subtitles. |
| Identity | Insufficient/conflicting/stale evidence | Ask for manual character selection/name. |
| Memory | Migration, busy/corrupt/index mismatch | Preserve source data; disable vector retrieval or repair index. |
| Worker | Crash, timeout, heartbeat, repeated failure | One bounded restart, then feature quarantine/degradation. |
| Cancellation | Superseded/late event | Ignore safely; no user action unless conversation remains stuck. |
| Internal | Unclassified invariant failure | Stop affected stage, preserve correlation ID, export sanitized diagnostics. |

Errors expose retryability, affected stage, active fallback, correlation ID, and safe action. They never expose credentials, prompt/memory contents, raw provider payloads, or private paths.
