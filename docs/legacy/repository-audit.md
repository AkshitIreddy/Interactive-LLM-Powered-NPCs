# Version 1 repository audit

Status: evidence baseline for the 2.0 rewrite  
Audit date: 2026-08-28  
Audited revision: `503ef3b64a921b6a11efa9e3e0432a0c3de3b619` on `main`

## Executive finding

Version 1 is a valuable product prototype, not a safe or maintainable production base. It proves the core loop—speak to a visible or named NPC, retrieve lore and memory, generate an in-character reply, synthesize a voice, and optionally animate the face—but every latency-sensitive stage is serialized in a notebook and most state is stored in mutable files under the source tree.

The 2.0 decision is therefore a clean replacement. Text lore and other user-authored, inspectable data may be imported after provenance review. Python orchestration, generated code execution, Chroma indexes, face-representation pickles, SadTalker, stored conversations, and plaintext credentials are not executable migration inputs.

## Audit method and checkout condition

The audit inspected all tracked root code, notebooks, game data, the single-monitor variant, dependency declarations, generated stores, and the vendored SadTalker tree. It did not run the v1 interaction loop because four response generators write model output into `temp.py` and execute it.

At audit time the checkout contained widespread modifications. `git diff --ignore-space-at-eol --quiet` returned success, proving the tracked modifications then present were end-of-line-only. They were preserved rather than normalized or reverted. This fact describes the observed checkout only; it is not a blanket promise about later changes.

Inventory at the audited revision:

| Item | Observed |
| --- | ---: |
| Tracked files | 332 |
| Tracked `SadTalker/**` files | 190 |
| Tracked files in root Cyberpunk tree | 52 |
| Tracked files in `Games/Cyberpunk_2077/**` | 52 |
| Repository working size | about 216 MiB |
| SadTalker working size | about 72 MiB |
| Each Cyberpunk tree | about 28 MiB |
| Chroma/face pickle files across both game trees | 24 |
| Chroma `.bin`/`.parquet` files across both trees | 18 |

The two Cyberpunk trees are near-duplicates: 51 of 52 tracked paths have the same blob at the audited revision; `Johnny_Silverhand/pre_conversation.json` differs. This is particularly dangerous because the notebook addresses the root tree while the README can lead maintainers toward `Games/`, so edits can silently land in the inactive copy.

## Runtime architecture observed

The root `main.ipynb` owns configuration, capture, input, state flags, UI drawing, orchestration calls, playback, and compositing in one infinite loop. `functions/main.py` selects one of four generation functions. Each generator independently loads files and services, constructs a Cohere/LangChain chain, mutates conversation JSON, synthesizes audio, and—on the visual path—waits for SadTalker to render a complete MP4.

There is no installed desktop application, daemon, worker supervisor, process protocol, stable schema, dependency lock, automated migration, test suite, build pipeline, crash recovery, installer, updater, or diagnostic interface. The only dependency manifest is an unpinned `requirements.txt` with ten top-level package names.

## Findings by severity

### Critical

1. **Model output becomes Python source.** `audio_generate_side_character.py`, `audio_generate_background_character.py`, `video_generate_side_character.py`, and `video_generate_background_character.py` interpolate the untrusted LLM reply into a triple-quoted Python program, save it as root-level `temp.py`, and run `.venv/Scripts/python.exe temp.py`. Quotes or crafted response text can break out of the string and execute code as the user.
2. **Plaintext credentials are source-tree state.** `apikeys.json` is loaded by token counting, embeddings, retrieval, summarization, personality creation, and response generation. Rotation across a random key list is not secret storage, access control, redaction, or rate-limit handling.
3. **Untrusted pickle state is committed.** DeepFace representation caches and Chroma index metadata use pickle. Loading a tampered pickle can execute code. No v2 importer may deserialize these files.
4. **Sensitive perception is implicit.** Every turn opens webcam index 0, captures a frame, writes it to `temp/webcam_photo.jpg`, and runs emotion analysis. The background-video path also infers age, gender, and race and uses those guesses to generate identity/personality. There is no consent flow, privacy boundary, retention control, or reliable cleanup on failure.

### High

1. **The live path is strictly serial.** Capture and disk writes precede face detection; emotion capture precedes identity; two independent vector lookups and the full LLM response precede TTS; complete TTS precedes complete SadTalker generation; only then does playback begin. There is no streaming, cancellation, deadline, barge-in, warm-worker contract, or resource scheduling.
2. **Single-monitor rendering is self-referential.** The app captures a game/desktop into an opaque OpenCV window and displays that window on the same monitor. The foreground mirror can cover the game, capture itself, consume focus, and become the only place where hotkeys work. See `single-monitor-root-cause.md`.
3. **Mutable state is shared through fixed paths.** All turns use `temp/screen.jpg`, `temp/extracted_face.jpg`, `temp/webcam_photo.jpg`, `temp/audio.wav` or `.mp3`, `temp/facial_animation.mp4`, `video_temp/`, and `temp.py`. Concurrent turns, recovery after a crash, or two sessions can corrupt one another.
4. **The UI and runtime share one blocking loop.** Microphone listening, cloud calls, model loading, file I/O, SadTalker subprocesses, and audio joins block the display thread. Failure can freeze the only interface.
5. **Subprocess success is assumed.** `subprocess.run` return codes are ignored. SadTalker output is selected as the first directory entry and moved from a predicted filename. Missing output, stale output, partial output, and concurrent runs are indistinguishable.
6. **Conversation state is committed before delivery.** The player line is written before generation and the NPC line before playback. If TTS, animation, or playback fails, memory claims dialogue occurred when it was never heard.

### Medium

1. **Character identity is weak and unstable.** The highest-confidence face in one frame is used; each known-character directory is searched independently with no global threshold policy, multi-frame track, OCR/subtitle evidence, or interaction lock. A result of `NULL` immediately turns the face into a background NPC.
2. **Name fallback is brittle.** It tests exact whole-utterance matches and whether the transcript starts with any name component. It misses ordinary mentions and can collide on common first names.
3. **Memory mixes authority levels.** Mutable dialogue, generated summaries, character knowledge, and public lore feed prompts without provenance or spoiler/quest boundaries. The default background character reuses one mutable slot and rotates on a ten-minute timestamp rather than a stable encounter identity.
4. **Token counting and retrieval repeatedly create cloud clients.** Helpers reread credentials and instantiate Cohere embeddings or LLMs. The code has no persistent client pool, retry budget, cancellation, or request correlation.
5. **Media handling is lossy and brittle.** Frames repeatedly cross disk and color conversions; synthesized audio alternates between WAV and MP3 while playback is hard-coded to `temp/audio.wav`; the pasted video frame replaces a rectangular region without an alpha/motion/occlusion model.
6. **Display assumptions are fixed.** Both notebooks hard-code 1920×1080 and screen origin `(0,0)`. They do not account for negative monitor origins, per-monitor DPI, HDR, resize, ultrawide, window/client bounds, occlusion, or graphics-device loss.
7. **Dependencies and provenance are not reproducible.** Package versions, Python, CUDA, PyTorch, model revisions, hashes, and license/attribution data are absent. SadTalker is copied wholesale rather than pinned as an external build input.

## Data and artifact classification

| Class | Examples | 2.0 treatment |
| --- | --- | --- |
| Human-readable source candidates | `world.txt`, `public_info.txt`, `bio.txt`, `character_knowledge.txt`, talking-style JSON | Quarantine, provenance review, transform through a deterministic importer, then validate against `GameProfileV2`. |
| User/session state | `conversation.json`, default NPC name/voice/timestamp, generated bio | Do not ship. Offer explicit opt-in transcript import only after validation and secret/PII review. |
| Derived rebuildable indexes | Chroma parquet/bin/pickle, DeepFace representation pickle | Never deserialize. Rebuild embeddings from accepted text and identity evidence. |
| Potentially copyrighted media | character reference JPEGs, game-derived faces | Do not redistribute without verified permission; replace with user-local or synthetic evidence. |
| Runtime/vendor source | notebooks, `functions/`, `SadTalker/`, per-character `voice.py` | Archive for historical reference; never load on the 2.0 path. |
| Secrets | `apikeys.json` and any values copied to notebooks/logs | Revoke/rotate externally, remove from distribution/history as separately approved, and store future secrets in Windows Credential Manager. |

## What the prototype proved

The audit does not discard the successful product ideas:

- push-to-talk and an audio-only fallback make the concept usable before visual integration is perfect;
- important characters and background NPCs need different identity and personality strategies;
- world lore, character biography, speaking style, recent dialogue, and long-term memory are distinct useful inputs;
- important characters need stable voice mappings while background NPCs need deterministic diversity;
- optional emotion and animation can add presence when they are isolated from the reliable speech path;
- a visible response overlay and status feedback are necessary while playing.

These ideas become explicit, versioned contracts in 2.0 rather than implicit file conventions.

## Exit criteria for the legacy tree

The legacy runtime may be removed from normal builds only after:

1. an immutable archive or Git reference preserves the audited revision;
2. every candidate text asset has an import disposition and provenance status;
3. no v2 build/test/package command imports a legacy Python module or deserializes a legacy pickle/index;
4. credential rotation is recorded outside the repository without copying secret values into documentation;
5. deterministic fixtures cover the product behaviors retained from v1.

