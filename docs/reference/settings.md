# Settings reference

All settings follow `global → game → character` inheritance where applicable. A child value can inherit or override; Reset restores inheritance rather than copying a stale parent value.

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
