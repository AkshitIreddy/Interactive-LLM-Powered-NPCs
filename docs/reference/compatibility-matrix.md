# Compatibility matrix

This matrix describes the authored **RC profile set**, not current live certification. All 20 documents pass the current schema/semantic validator, but current capability evidence must come from generated replay/certification records in the build under test.

| Profile | Authored target | Replay target | External/visual claim | Safety boundary |
| --- | --- | --- | --- | --- |
| Skyrim Special Edition | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| Cyberpunk 2077 | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| Baldur's Gate 3 | Full | Required | External audio/subtitles; visuals unverified | Single-player; co-op ambiguity refuses capture/overlay |
| Fallout 4 | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| Fallout: New Vegas | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| The Witcher 3 | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| Starfield | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| Bannerlord | Full | Required | External audio/subtitles; visuals unverified | Single-player campaign only |
| Kingdom Come: Deliverance II | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| Oblivion Remastered | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| The Sims 4 | Full | Required | External audio/subtitles; visuals unverified | Offline/local play |
| Stardew Valley | Full | Required | External audio/subtitles; visuals unverified | Single-player; multiplayer ambiguity refuses capture/overlay |
| Minecraft Java | Full | Required | External audio/subtitles; visuals unverified | Local single-player; no mod required |
| Divinity: Original Sin 2 | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| Mass Effect Legendary Edition | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| Dragon Age: Inquisition | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| Kenshi | Full | Required | External audio/subtitles; visuals unverified | Single-player; no mod required |
| RDR2 Story Mode | Full | Required | External audio/subtitles; visuals unverified | Story Mode only; protected/online ambiguity blocked |
| GTA V Story Mode | Full | Required | External audio/subtitles; visuals unverified | Story Mode only; protected/online ambiguity blocked |
| Elden Ring offline | Full | Required | External audio/subtitles; visuals unverified | Offline with protection absent; ambiguity blocked |
| Generic Game | Minimal/manual | Required fixture | Experimental only | Capturable single-player window |

“Full” refers to authored profile data, not equal integration depth. See [capability semantics](../getting-started/supported-games.md).

The authoritative evidence vocabulary is `unsupported`, `experimental`, `replay_verified`, and `live_certified`, recorded independently for each profile capability.

## Display matrix target

External capture/overlay certification covers single/multi-monitor, negative origins, 100/125/150/200% DPI, 720p–4K, 16:9/21:9/32:9, SDR/HDR, windowed/borderless, resize, alt-tab, monitor changes, and device loss. True exclusive fullscreen may be audio/subtitles only. No row claims current live-game certification or requires a mod.

## Provider and model compatibility

Catalog presence and implemented verification are separate. See [providers and models](providers-and-models.md) and the build's Diagnostics evidence.
