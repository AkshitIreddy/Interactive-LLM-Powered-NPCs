# Choose API routes and a performance mode

Provider routing and performance policy are separate decisions.

## Conversation routing

### API-powered

Use explicitly configured hosted services for speech recognition, language generation, speech, and optional semantic retrieval. This requires no local conversation-model download, but transmits the selected inputs/context under provider policies and may incur cost.

Each stage may use a different hosted provider. The UI must show which information crosses the network. A failed stage stays failed or uses a pre-authorized provider route; it never silently switches accounts or services.

### API + optional local lip-sync

Conversation still uses hosted APIs. The user may separately install one eligible generic screen-space lip-sync pack after reviewing its exact model/revision, download/storage size, RAM/VRAM, backend, license, measured quality/game impact, and experimental caveats. Packs are never automatic, and none is advertised as qualified yet.

### Offline

Disable provider traffic and new AI conversation. Local configuration, deterministic fixtures, diagnostics, and existing memory remain available. An installed lip-sync pack does not make offline conversation possible by itself.

## Performance mode

| Mode | Priority | Expected degradation behavior |
| --- | --- | --- |
| Competitive | Game frame time | Audio/subtitles first; nonessential vision and visual animation off. |
| Fast | Response latency | Low-latency provider routes and conservative visual work. |
| Balanced | Mixed | Default for supported hardware after simulation. |
| Immersive | Presence quality | More identity/animation work when budget remains. |
| Maximum Quality | Output quality | Higher resource use; still respects hard VRAM/FPS limits. |
| Custom | Explicit controls | User owns each limit; privacy/provider boundaries remain enforced. |

The fixed degradation order is: reduce continuous vision; disable screen-space lip-sync; neutralize optional emotion/style; use SQLite FTS/recent context without hosted semantic retrieval; continue selected-character audio; offer typed input/subtitles; surface a retryable LLM error.

## Recommendation

Start with push-to-talk, Balanced, and explicit API routes during onboarding simulation. Leave lip-sync off unless a qualified pack is available and its self-test passes with the target game running. No visual result should be inferred from GPU name alone.
