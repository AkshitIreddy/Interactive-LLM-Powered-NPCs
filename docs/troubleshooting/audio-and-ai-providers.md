# Audio and AI providers

## Microphone/PTT does not respond

Check Windows microphone privacy permission, selected device, input level, global-hotkey conflict, and PTT rehearsal. PTT key-up is the authoritative endpoint. Try another unreserved key/device and deterministic typed simulation.

## NPC transcribes itself

Use headphones, keep push-to-talk, verify input/output device separation, and disable VAD until echo cancellation is qualified. A new turn should cancel playback before recording continues.

## Poor recognition

Confirm language/model, microphone distance/gain, clipping, noise, and game volume. Compare a clean rehearsal and game-noise rehearsal. Do not present subjective success as a WER result.

## Provider authentication/rate limit

Run the provider connection test, check Credential Manager reference, account/quota/model access, endpoint/region, system clock, and provider status. Rotate the upstream key if exposed. Never paste it into logs.

## Response stops at a Spine stage

- **Transcribing:** inspect device/STT finalization.
- **Remembering:** expect FTS/recent-context degradation if vectors fail.
- **Responding:** inspect LLM route, quota, schema/safety result.
- **Voicing:** enable subtitles and inspect TTS format/voice.
- **Animating:** conversation should continue; visuals are optional.

No stage should silently switch providers. Choose a different route manually or configure an explicitly authorized fallback.

## Silent/garbled TTS

Check output device, exclusive-mode conflicts, sample format, volume, and subtitle content. Restart only the quarantined audio worker if Diagnostics offers it; do not restart the entire game first. Continue with subtitles while reporting audio health and chunk-gap data.
