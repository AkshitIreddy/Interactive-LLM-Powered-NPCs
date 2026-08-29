# Response Console UI architecture

## Information architecture

The consumer UI is organized around tasks, not implementation components:

| Area | Primary job |
| --- | --- |
| Home | Show readiness, selected game/character, Start/Stop and live Response Spine. |
| Games | Discover installations, choose/configure a profile, show exact capabilities and compatibility. |
| Characters | Inspect/select important and encountered NPCs; apply explainable per-character overrides. |
| Conversation | PTT/VAD, subtitles, verbosity, creativity, length, interruption and memory behavior. |
| Presence | Vision, identity, emotion, webcam and lip-sync with explicit privacy/resource disclosures. |
| Performance | Provider-route latency, performance preset, VRAM/FPS targets for optional visuals, reference/This PC benchmarks and degradation. |
| Models | Hosted provider/model selection plus explicit optional lip-sync pack choice, download/update/repair/removal, and resource/license details. |
| Diagnostics | Guided mic/speaker/provider/model/capture/game/overlay/latency checks and redacted export. |
| Settings | Global defaults, per-game/character inheritance, accessibility, logs, storage and privacy. |
| Help | Onboarding replay, plain-language concepts, compatibility, troubleshooting and documentation. |

Provider, STT, TTS, memory and overlay details appear contextually inside these tasks rather than as an intimidating first-level list. Advanced controls use progressive disclosure and remain searchable.

## Onboarding state model

```text
Welcome
  → hardware and privacy scan
  → API provider routes / Offline
  → game discovery and selection
  → provider credentials and model choices
  → microphone, speaker and PTT rehearsal
  → optional presence features (off by default where sensitive/expensive)
  → Competitive / Fast / Balanced / Immersive / Maximum Quality / Custom
  → deterministic Eclipse Harbor simulation
  → Ready
```

Each step is resumable and independently valid. Back navigation preserves entered non-secret state; secret fields display configured/not-configured, never the value. Users may skip optional presence and lip-sync downloads and still reach an audio/subtitle-capable configuration when valid hosted speech and language routes exist.

## Configuration model

Provider routing and performance are orthogonal:

- execution: API-powered conversation or Offline;
- performance: Competitive, Fast, Balanced, Immersive, Maximum Quality, Custom.

Each setting has a type, safe default, plain-language description, effect/privacy/resource tags, and inheritance source. Resolution order is `character override → game override → global setting → product default`. The UI always shows the effective value and its source; resetting removes only the selected override.

No preset secretly authorizes a new provider or data category. Changing to a preset that would require missing authority produces a review step.

## Response Spine contract

The runtime sends ordered stage events with turn ID, status, QPC timing, provider/route label, optional safe error and degradation reason. The UI renders only active/current-generation events. Stages can be:

`pending | active | streaming | complete | skipped | degraded | cancelled | failed`

Animation is not shown as successful when audio-only fallback is active. Timing detail is available without turning the primary Home view into a developer dashboard.

## Accessibility and resilience

- WCAG 2.2 AA contrast/focus/target-size expectations and Xbox Accessibility Guidelines inform acceptance.
- Full keyboard operation, visible focus, semantic landmarks/headings, Narrator labels/live regions, and controller-friendly target order are required.
- Support Windows high contrast, reduced motion, 100–200% scaling and narrow-window reflow.
- Smoked graphite and restrained teal are theme tokens, not hard-coded indicators; status always has text/icon semantics.
- Animations pause/reduce without hiding state changes.
- UI reload reconnects to the runtime snapshot and ordered event cursor; it never reconstructs session truth from local component state.
- Errors state what failed, what still works, the selected fallback, and a direct diagnostic/fix action.

## Visual acceptance

Unit/DOM/accessibility tests are necessary but not visual proof. Release-candidate screenshots and human review cover onboarding, Home ready/active, every major area, download progress/failure, diagnostics failure, dark/light/high-contrast, reduced motion, narrow window and 200% scaling. Claims about layout/quality require rendered evidence.

References:

- WCAG 2.2: <https://www.w3.org/TR/WCAG22/>
- Xbox Accessibility Guidelines: <https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/>
- React Aria accessibility: <https://react-spectrum.adobe.com/react-aria/accessibility.html>
