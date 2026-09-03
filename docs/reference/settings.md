# Settings reference

All settings follow `global → game → character` inheritance where applicable. A child value can inherit or override; Reset restores inheritance rather than copying a stale parent value. The native snapshot reports the winning value, exact source scope, and whether it came from a preset, an explicit override, or inheritance. Saves are revision-checked and atomically persisted; a stale writer is rejected instead of overwriting a newer choice.

## Preset axes and defaults

The 2.0 design deliberately replaces the old single list of Cloud, Hybrid, Fully Local, Performance, and Immersive profiles with two independent axes:

- **Execution:** Cloud/API-first, Hybrid, Fully Local.
- **Performance:** Competitive/Minimum Overhead, Fast, Balanced, Immersive, Maximum Quality, Custom.

This prevents a performance choice from silently changing network egress or provider routes. First run is **Cloud/API-first + Balanced**. Push-to-talk, subtitles, and memory are on; overlay, vision, and webcam-presence consent are off.

| Performance preset | Verbosity | Creativity | Response | Interruption | Input | Emotion | Vision |
| --- | --- | ---: | --- | --- | --- | --- | --- |
| Competitive / Minimum Overhead | Concise | 25 | Short | Immediate | PTT | Off | Off |
| Fast | Concise | 35 | Short | Finish sentence | PTT | On | Off |
| Balanced | Standard | 50 | Medium | Finish sentence | PTT | On | Off |
| Immersive | Detailed | 65 | Long | Finish sentence | VAD intent | On | On |
| Maximum Quality | Detailed | 75 | Long | Disabled | VAD intent | On | On |
| Custom | Standard | 50 | Medium | Finish sentence | PTT | On | Off |

Every row keeps subtitles and memory on and overlay off. Webcam presence remains off for every preset; it requires its own explicit override. A VAD, vision, emotion, or webcam value is configuration intent only and cannot construct or activate a producer.

### Egress and fallback authority

| Execution preset | Transcript | Microphone audio | Captured game image | Local memory context |
| --- | --- | --- | --- | --- |
| Cloud / API-first | Selected provider route only | Selected provider route only | Denied | Selected provider route only |
| Hybrid | Selected provider route only | Selected provider route only | Selected provider route only | Selected provider route only |
| Fully Local | Denied | Denied | Denied | Denied |

“Selected provider route only” is a constraint, not proof that a route exists and not a network permission minted by the preset. The adjacent `routeSnapshot` is the native evidence reference and may be absent. The provider-loadout turn snapshot remains authoritative for the exact provider, model, transmitted data classes, and credential reference. Likewise, a local selection is not admitted until a native resource-admission receipt exists. Preference mutation always reports `activationPerformed: false`, `mutationActivatedRoutesOrPacks: false`, and `automaticProviderFallback: false`.

Provider fallbacks are separate saved loadout entries. They must be explicitly user-authorized and `ManualOnly`; neither a preset switch nor a fault can silently change provider or egress class.

### Persistence and migration

- A first native preferences document is initialized from persisted onboarding choices, atomically materialized immediately, and reports `migratedLegacyOnboarding` for that process.
- A valid saved v1 document reopens as `current` with the same revision and scoped entries.
- An invalid, oversized, symlinked, or structurally invalid document is ignored and reports `recoveredDefaults`; safe API-first + Balanced defaults are used in memory without activating a route or pack.
- Reset requires an explicit second confirmation. Global reset restores safe defaults; game and character reset remove that scope so inheritance resumes.
- Legacy screen-presence intent may migrate to vision intent, but never grants webcam consent.

## Session

- **Conversation routing:** API-powered or Offline.
- **Performance mode:** Competitive, Fast, Balanced, Immersive, Maximum Quality, Custom.
- **Input:** push-to-talk default, optional validated VAD, typed fallback.
- **Output:** device, volume, subtitles, interruption behavior.
- **Character targeting:** profile identity, manual selection/name, confidence timeout.

## Providers

STT/LLM/TTS/retrieval route, model, credential reference, endpoint/region where supported, language, and explicitly authorized fallback. Named loadouts can group these choices with global/game/character inheritance. Every saved fallback is `ManualOnly` and `user_authorized`; switching is explicit, and changing a route or loadout always exposes data/cost consequences.

## Conversation

Spoiler tier, response length/style bounds, barge-in, subtitle persistence, memory scope, and relationship continuity. Model text cannot introduce game actions.

## Presence

Game capture, identity sources, OCR regions, webcam presence, and experimental generic screen-space animation. Webcam and experimental visuals default off. Active sensing is visible.

## Performance

Target game FPS/frame-time impact, VRAM ceiling/headroom, model residency, continuous-vision rate, visual-animation budget, and degradation notifications. Current available resources override unsafe requests.

## Models

Hosted provider model choices and optional generic lip-sync pack version/channel, device/backend, storage location, update consent, retention/rollback, and cache limit. No conversation-model packs are downloaded. Lip-sync license acceptance and download consent are per exact pack/version.

## Privacy and diagnostics

Offline/network policy, telemetry level (content-free by default), diagnostic retention, crash-dump consent, and export preview. Provider service retention is governed upstream.

## Accessibility

Text/UI scale, high contrast, reduced motion, captions, keyboard/controller bindings, and notification behavior. Settings remain operable at 200% scale.

Exact serialized names/defaults are generated from stable schemas once frozen; this page documents behavior and must not substitute invented keys.

## Fault and recovery evidence boundary

The deterministic simulator and focused runtime tests exercise recovery invariants, but they are not installed-device certification:

| Fault | Current connected evidence | Invariant / recovery | Installed-only gap |
| --- | --- | --- | --- |
| Hosted provider error or timeout | Deterministic provider failure and explicit-retry scenarios; typed runtime degradation | No audio/delivery commit from the failed generation; retry is a new manual generation with the same pinned route unless the user explicitly changes it | Live provider outage, billing, rate-limit, and retry UX matrix |
| Runtime crash | Deterministic restart scenario | Pre-crash generation cannot commit; post-restart work uses a new generation | Packaged process-kill and recovery timing |
| GPU/VRAM pressure | Deterministic low-VRAM scenario and native admission policy | Disable optional continuous vision, then screen-space lip-sync; preserve audio/subtitles; never switch to cloud | Live game-load DXGI/NVML pressure and frame-impact evidence |
| Lip-sync worker crash or stale patch | Deterministic crash/stale-frame scenarios and native fail-open contracts | Quarantine/ignore visual work; leave the captured frame untouched; voice/subtitles continue | Qualified installed compositor/model crash matrix |
| Identity or memory worker failure | Simulator supports quarantine semantics; identity-engine fixtures cover ambiguity/offscreen continuity | Do not guess or silently switch; conversation may continue through explicit selection and bounded memory fallback | Connected vector-worker fault fixture and installed identity worker failure UX |
| Microphone/STT | Runtime/provider adapters fail closed and typed input remains available | Never invent a transcript or change STT provider automatically | Real device loss/PTT/VAD/permission/reconnect matrix |
| TTS/audio output | Runtime route tests require typed TTS degradation and no false delivery receipt | Subtitles remain the dependable fallback; no provider switch or memory commit without truthful delivery evidence | Physical endpoint removal/loopback/reconnect matrix |
| Game/capture loss | Native target/capture bindings reject stale PID/HWND/frame evidence | Stop visual authority and retain explicit/manual character continuity; no injection or guessed capture | Installed game exit/relaunch, alt-tab, display/device-loss matrix |

R31 therefore remains an installed-product acceptance item even when deterministic tests are green. Faults may degrade or require a visible manual retry; they never authorize a hidden provider, egress, model-pack, or local-device fallback.
