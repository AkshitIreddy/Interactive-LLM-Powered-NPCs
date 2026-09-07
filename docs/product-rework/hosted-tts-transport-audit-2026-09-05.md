# Hosted TTS transport audit — 2026-09-05

This audit covers the executable WebSocket boundary for Cartesia, Deepgram, and
Inworld. Catalog presence alone does not qualify a route. A route becomes
selectable only after the real provider adapter, concrete transport, selected
model, selected stock voice, and PCM stream pass the headless live test.

## Primary protocol evidence

- Cartesia documents a persistent WebSocket at `/tts/websocket`, API-version
  authentication, context-scoped generation, raw PCM chunks, word and phoneme
  timestamp envelopes, completion, and cancellation. Each context response
  echoes `context_id`; `flush_done` is only a manual-flush acknowledgement with
  `done: false`, while the context is terminal only at `done: true`:
  <https://docs.cartesia.ai/api-reference/tts/websocket>,
  <https://docs.cartesia.ai/use-the-api/tts-websocket/contexts>,
  <https://docs.cartesia.ai/use-the-api/tts-websocket/context-flushing-and-flush-i-ds>
- Deepgram documents Flux at `/v2/speak`, the `Speak`, `Flush`, and `Interrupt`
  client messages, binary audio, and a `SpeechMetadata` terminal event. Aura 2
  remains on `/v1/speak` with `Flushed` terminal behavior:
  <https://developers.deepgram.com/docs/flux-tts/quickstart>,
  <https://developers.deepgram.com/docs/flux-tts/client-messages>,
  <https://developers.deepgram.com/docs/flux-tts/server-messages>
- Inworld documents reusable bidirectional contexts at
  `/tts/v1/voice:streamBidirectional`, `CreateContext`, `SendText`, `Flush`, and
  `Close`, raw PCM output, and asynchronous word/phonetic timing. Every result
  carries its `contextId`; `flush_context` can be embedded in the final
  `send_text`, and one `flushCompleted` event is emitted for each flush in
  request order:
  <https://docs.inworld.ai/api-reference/ttsAPI/texttospeech/synthesize-speech-websocket>,
  <https://docs.inworld.ai/tts/capabilities/timestamps>

The public Cartesia model guide currently describes Sonic 3.5 while the account
used for qualification accepted `sonic-3.6`. The implementation therefore
pins Sonic 3.6 only to the exact live-qualified route instead of treating an
undocumented family of IDs as supported.

## Live wire results

All measurements used the same short sentence, raw mono signed 16-bit PCM at
24 kHz, persistent authenticated sockets, and no playback. These figures are
provider-wire evidence and do not include the native app's capture, LLM, STT,
or playback path.

| Route | First PCM, warm | Completion | Timing metadata |
| --- | ---: | ---: | --- |
| Cartesia Sonic 3.6, Greg stock voice | 92.6 / 108.1 ms | 426.7 / 410.7 ms | 30 phonemes; no provider viseme symbols |
| Inworld TTS-2 Flash, Dennis stock voice | 392.0 / 376.7 ms | 899.5 / 591.9 ms | 44 phonemes and 44 provider visemes |
| Inworld TTS-2, Dennis stock voice | 403.5 / 401.7 ms | 1013.7 / 841.7 ms | 44 phonemes and 44 provider visemes |
| Deepgram Flux v2, Miles stock voice | 336.9 / 334.2 ms | 1747.4 / 1680.7 ms | no phoneme or viseme timing |
| Deepgram Aura 2 v1, Arcas stock voice | 314.7 / 314.2 ms | 1146.3 / 990.5 ms | no phoneme or viseme timing |

Immutable local reports:

- `E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives\provider-websocket-warm-20260905T043716Z.json`
- `E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives\provider-websocket-warm-20260905T043843Z.json`
- `E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives\provider-websocket-warm-20260905T044720Z.json`
- `E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives\provider-cancellation-reuse-20260905T044304Z.json`

Cartesia cancellation stopped audio after the first 7,392 PCM bytes, produced
`done` 26 ms later, and allowed a distinct next context on the same socket.
Inworld `close_context` did not stop generation promptly, so it is not treated
as a provider cancellation primitive. Local cancellation generation and late
event discard remain authoritative for every route.

## Implementation decisions

- Cartesia is pinned to `sonic-3.6`, stock voice Greg
  (`a0e99841-438c-4a64-b679-ae501e7d6091`), API version `2026-03-01`, and raw
  mono 24 kHz PCM. Other IDs fail closed until independently qualified.
- Cartesia uses a one-slot exclusive pool. Only a socket that emitted terminal
  `done` and was closed by the completed session returns to the pool. Cancel,
  provider error, protocol ambiguity, timeout, and dropped sessions destroy the
  lease. Idle sockets expire after a bounded interval.
- Each sentence-level pool lease appends a monotonic synthesis nonce to the
  provider-only context ID. Two sentences in one public turn therefore cannot
  collide on a reused socket. Every context-bearing Cartesia response,
  including provider errors, must match that exact wire context. A documented
  connection-level error without a context is accepted as a socket failure and
  the socket is destroyed rather than returned to the pool.
- Cartesia `flush_done` remains inside the receive operation as a nonterminal
  acknowledgement. Only `done` completes the utterance and makes the clean
  socket eligible for pooling.
- `finish` sends the final text/flush and returns without waiting for audio.
  Audio remains pull-streamed through `next_event` as soon as the provider emits
  it.
- Inworld binds one local session to one provider `contextId`, checks the
  `contextCreated` acknowledgement and every subsequent result against it, and
  waits for readiness before sending text. Intermediate semantic clauses omit
  `flush_context`; the final buffered clause embeds it, or one standalone
  `Flush` closes an utterance whose clauses were already submitted. This yields
  exactly one final flush and one terminal `flushCompleted`. Audio streams
  immediately, while provider word, phone, and viseme timing is retained only
  when its indices and timestamps are bounded and monotonic. Asynchronous
  timing may trail PCM and must never be moved backward in playback.
- Deepgram v2 ignores the nonterminal `Flushed` event and completes only on
  `SpeechMetadata`; v1 completes on `Flushed`. `SpeechInterrupted` and
  `ConfigureFailure` cannot be reported as successful completion.
- Endpoints, query names, authentication schemes, frame sizes, message sizes,
  connect/read/write/close deadlines, timeline arrays, decoded audio, and
  pre-readiness event queues are bounded. Readiness and receive operations use
  one absolute deadline across progress frames, acknowledgements, ping/pong,
  and other non-audio traffic. Provider error text and dialogue never enter
  Debug or typed errors.
- Cancellation gives a provider control message at most 50 ms before closing
  the transport. The close remains the authoritative barrier: cancelled
  sessions emit one local interruption and cannot deliver late provider PCM or
  return their socket to the Cartesia pool.

The native receipt must distinguish fresh upgrade latency from pooled reuse.
Warm provider measurements cannot be presented as the first-turn app latency.

## Production Rust promotion gate

The exact production adapters and concrete transports subsequently passed a
live headless qualification. This is stronger evidence than the protocol probe
above because it exercises credential resolution, session construction,
semantic buffering, transport decoding, cancellation, event conversion, and
the Cartesia pool together.

| Exact production route | First PCM | Result |
| --- | ---: | --- |
| Cartesia, fresh socket | 607.3 ms, including 390.6 ms upgrade | 115,200 PCM bytes in 15 chunks, 30 phonemes, completed |
| Cartesia, same public identity and pooled socket | 115.2 ms | 122,880 PCM bytes in 14 chunks, distinct synthesis context, completed |
| Cartesia, fresh after active cancel | 402.6 ms, including 266.7 ms upgrade | old context interrupted after 7,392 bytes; new context isolated and completed |
| Inworld Flash/Dennis, fresh | 1564.1 ms | 129,600 PCM bytes, 17 word alignments, 44 provider visemes, completed |
| Deepgram Aura 2/Arcas, fresh | 2092.4 ms | 126,722 PCM bytes in 67 chunks, completed; live latency outlier |

The production report is
`E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives\rust-production-adapter-qualification-20260905T050606Z.json`.
Its credential scan passed. These figures still stop at the provider adapter;
at this promotion step the RuntimeTtsBridge and playback path still needed
separate receipts. The next section records the later Rust bridge boundary;
native broker playback and physical output remain unqualified.

## Structured LLM to TTS streaming qualification

One additional bounded call connected the production selected Groq
`RuntimeBridge` for `qwen/qwen3.6-27b` in its qualified non-reasoning profile to
the production Cartesia `RuntimeTtsBridge`. The runtime
`StreamingResponseAdapterV1` released the complete validated
`spoken_response` field and dispatched one 12-word sentence while the LLM
stream was still open. Cartesia PCM was consumed in memory; no WAV, OS audio
endpoint, native broker receipt, or physical delivery claim was produced.

| Boundary | Measured latency |
| --- | ---: |
| LLM request to first provider delta | 252.534 ms |
| LLM request to validated spoken field | 252.777 ms |
| Validated spoken field to provider first PCM | 240.123 ms |
| Validated spoken field to RuntimeTtsBridge first PCM | 240.187 ms |
| End-to-end request to RuntimeTtsBridge first PCM | 492.964 ms |

The spoken field crossed the gate 1.514 ms before the LLM stream finished and
1.640 ms before the complete envelope validation returned. Within the Cartesia
component, the fresh WebSocket session took 154.851 ms to become ready, the
ready session took another 85.014 ms to emit provider PCM, and the bridge added
64 microseconds before exposing its first PCM item. The bridge consumed 172,800
PCM bytes in 40 chunks with 55 alignment events and completed 932.207 ms after
the original LLM request.

The redacted create-new receipt is
`E:\temp\InteractiveNPCs\qualification\2026-09-05-groq-qwen-cartesia-speech-first.json`.
The runner scanned both process output and the receipt for the exact supplied
credentials and found zero matches. This is a component-chain qualification of
the production adapters and runtime speech gate. It is not a normal
`HostState`, native broker, or audible game-path qualification because those
paths correctly require authenticated playback leases.

Final source validation after the production qualification:

- `cargo test -p npc-providers-tts --tests`: 64 passed, with two explicitly gated live
  tests ignored by default.
- `cargo check -p npc-providers-tts --all-targets`: passed.
- `cargo clippy -p npc-providers-tts --all-targets --all-features --locked
  --offline -- -D warnings`: passed with the restored Windows Clippy 0.1.96
  component.
- `cargo fmt -p npc-providers-tts -- --check` and `git diff --check`: passed.

Lint-only corrections replaced panic-prone shorthand in deterministic test
fixtures with labeled expectations. No provider call was repeated for those
source-neutral changes.
