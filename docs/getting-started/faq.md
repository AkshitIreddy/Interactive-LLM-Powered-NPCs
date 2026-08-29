# Frequently asked questions

## Can I install 2.0 now?

No public installer or RC exists. The repository is a development preview. Developers can run implemented source components; a private local RC will be prepared for review before any public release decision.

## Does it work with every game?

No. The 20 target profiles have authored game knowledge and detection/safety data, but capabilities are certified independently. Generic Game provides manual-target conversation/audio/subtitles for suitable capturable single-player games; automatic identity, world state, and lip-sync are not universal. Native game actions are not part of the product.

## Does it modify or inject into my game?

No. The supported path uses external Windows capture/composition and data-only profiles. The application does not require or install mods, script extenders, hooks, injected DLLs, or per-game executable adapters. Protected/online/anti-cheat ambiguity is blocked, not bypassed.

## Must the NPC's face be visible?

No. Select or say the intended character name and continue through audio/subtitles and memory. Visual animation is optional.

## Is a webcam required?

No. Presence/webcam processing is local, ephemeral, visibly active, off by default, and never used for demographic inference.

## Do I need to download local AI models?

No. Conversation is API-first: LLM, STT, TTS, and retrieval use only providers the user explicitly configures. The only optional local AI downloads contemplated are generic screen-space lip-sync packs. They are never automatic, and each candidate must disclose model, download/storage size, VRAM/RAM needs, license, measured quality, and experimental limitations before installation.

## Which cloud providers are supported?

The catalog models several hosted LLM, STT, and TTS providers. Only the current build's adapter self-test establishes working support. Routes and credentials are explicit; compatibility endpoints are best-effort.

## Will it clone a game's actor voices?

No. Default voices use provider-neutral characteristics and properly licensed services/models. Performer imitation or cloning extracted game audio is not accepted without documented rights and consent.

## Does it remember everything forever?

No. Canon, turns, episodic memory, relationships, and temporary game state are separate. Only delivered dialogue can be proposed for memory, with validation/provenance/retention controls. Users must be able to inspect and delete local memory.

## Are the latency/FPS goals already achieved?

Not as published results. They are RC acceptance targets. Every future claim must link to raw traces and a complete environment manifest.

## Why not run the old notebooks?

The prototype is synchronous and includes plaintext keys, unsafe pickle data, executable per-character Python, and model-output-to-code execution. It is unsupported migration evidence, not a 2.0 fallback.
