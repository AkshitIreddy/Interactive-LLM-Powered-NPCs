# Hosted stock-TTS timing alternatives

This bounded harness compares the hosted stock-voice routes that can replace a
slow TTS provider in the native runtime. It sends one innocuous sentence,
serially, to each explicitly selected provider. It never prints or persists a
credential, provider response body, account identifier, or request identifier.

The comparable output is mono signed 16-bit little-endian PCM at 24 kHz. Each
run records request start to response headers, first response body bytes, first
decoded PCM, and completion, followed by audio duration, RMS, peak, clipping,
and a content hash. A `.pcm` artifact is retained only when the response is
non-silent. Repeat count defaults to one. Use `--repeat 2` only when a cold
result needs a same-connection steady-state comparison.

Every report path is immutable. By default the scripts add a UTC timestamp;
`--report-file` can supply a unique path explicitly and refuses to overwrite it.

```powershell
python scripts/provider-timing-alternatives/provider_timing_alternatives.py `
  --credentials 'C:\Users\akshi\Desktop\Code Palace\Commonly used Keys.txt' `
  --providers deepgram,inworld `
  --output-dir 'E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives'
```

Use `--dry-run` to list planned routes and credential-label presence without
making API calls. `--probe-accounts` currently performs only ElevenLabs' GET
subscription call; it does not synthesize audio or consume credits. Use
`CARTESIA_VOICE_ID` to qualify another stock voice. The default ID was verified
through Cartesia's official voice endpoint as **Greg - Supporter**, a public
masculine English US stock voice.

`provider_websocket_warm.py` performs two turns over one persistent connection,
with connection and Inworld context setup reported separately. It measures the
first WebSocket message, first decoded PCM, first playable 20 ms, completion,
audio quality, frame sizes, and alignment event counts. These two samples expose
gross cold/steady differences; they are integration measurements rather than
service latency guarantees. `provider_cancellation_reuse.py` tests active
cancellation or closure and then proves a second context on the same connection.
`build_evidence_index.py` hashes the immutable reports and verifies that no
resolved credential value appears in any evidence artifact.

`run_rust_adapter_qualification.py` is the final promotion gate. It injects
credentials into a temporary child-process environment and runs the production
Rust `Provider` and `WebSocketTransport` implementations through the standalone
path-dependent harness in `rust-adapter-qualification`. Its timings start before
`start_session`, so they include authenticated WebSocket upgrade and provider
context setup. The report contains neither the fixture text nor credentials.

## Live results from 2026-09-05

All successful routes produced non-silent, unclipped mono signed 16-bit
little-endian PCM at 24 kHz for the same sentence. Persistent connection setup
is excluded from the turn timings below.

| Provider route | First decoded PCM, turns 1 / 2 | Complete, turns 1 / 2 | Alignment |
| --- | ---: | ---: | --- |
| Cartesia Sonic-3.6, Greg | 92.6 / 108.1 ms | 426.7 / 410.7 ms | 30 timed phonemes; no provider viseme IDs |
| Deepgram Flux, Miles | 336.9 / 334.2 ms | 1747.4 / 1680.7 ms | none |
| Deepgram Aura-2, Arcas | 314.7 / 314.2 ms | 1146.3 / 990.5 ms | none |
| Inworld TTS-2 Flash, Dennis | 382.0 / 363.4 ms | 515.3 / 507.7 ms | disabled |
| Inworld TTS-2 Flash WORD/ASYNC, Dennis | 392.0 / 376.7 ms | 899.5 / 591.9 ms | 44 phonemes and 44 visemes |
| Inworld TTS-2, Dennis | 406.7 / 415.8 ms | 1000.4 / 827.5 ms | disabled |
| Inworld TTS-2 WORD/ASYNC, Dennis | 403.5 / 401.7 ms | 1013.7 / 841.7 ms | 44 phonemes and 44 visemes |

Cartesia is the measured latency leader and should be the first hosted fallback.
Its phoneme events can drive the native mouth controller through one explicit
phoneme-to-mouth map. Inworld Flash WORD/ASYNC is the strongest direct lip-sync
option because each phone includes a provider `visemeSymbol`; alignment-only
`audioChunk` envelopes can trail the last non-empty audio chunk, so the adapter
must continue through `flushCompleted`. Deepgram Flux is the strongest barge-in
option because `Interrupt` returns `SpeechInterrupted`; Aura-2 completed faster
than Flux in these two turns when conversational interruption was not needed.

The cancellation check sent Cartesia's cancel command after the first 7,392 PCM
bytes and observed no later audio before `done`; a second context on the same
socket remained usable. Cartesia's current contexts guide only guarantees
halting work that has not begun generating, so the runtime must still stop
playback immediately and discard late frames by turn epoch. Inworld
`close_context` is a flush-and-close operation rather than cancellation: the
live check received another 118,096 PCM bytes before `flushCompleted` and
`contextClosed`; a new context on the same socket then succeeded.

## Production Rust adapter promotion gate

The final live run exercised the actual production adapters after their route
validation and pooling code stabilized:

- Cartesia's first completed session opened one fresh connection and reached
  PCM in 607.3 ms, including a 390.6 ms authenticated upgrade. A second session
  with the same public `SessionIdentity` leased that socket in 0.004 ms and
  reached PCM in 115.2 ms. Both completed with exact PCM and 30 timed phonemes.
- A third same-identity session was cancelled after its first 7,392 PCM bytes.
  It returned the provider-neutral `Interrupted` event, closed its terminal
  stream, and did not return the cancelled connection to the pool. The next
  same-identity session opened a fresh socket and completed with exact PCM. The
  final counters were two fresh connections, two reused connections, and zero
  stale evictions. Successful live response validation also proves that the
  provider echoed the per-lease `:synthesis-N` wire context rather than a prior
  context.
- Inworld's production Flash/Dennis route completed with 129,600 PCM bytes in
  six chunks, 17 word-alignment items, and 44 provider visemes.
- Deepgram's production Aura-2/Arcas route completed with 126,722 PCM bytes in
  67 chunks. That fresh live call was a slow outlier at 2,092.4 ms to first PCM
  and 9,952.2 ms to terminal completion; the earlier two-turn protocol probe was
  314.7/314.2 ms to first PCM. This reinforces keeping provider fallback and
  live timing receipts rather than assuming catalog latency.

## Adapter recommendation

The native adapter should expose a provider-neutral stream of timestamped
`pcm_s16le` frames and optional alignment events. It should keep one persistent
connection per active provider, bind cancellation to an exact turn epoch, stop
accepting chunks after cancellation, and report separate submitted,
first-decoded-frame, queued, and drained receipts. Inworld alignment must remain
optional: its `ASYNC` strategy preserves first-audio latency while trailing
phoneme/viseme records can drive the source-preserving mouth controller. A
provider result is usable only after decoded non-silent PCM exists; HTTP 200,
headers, or catalog presence alone are not audio proof.

## Current primary-source ledger (checked 2026-09-05)

1. [OpenAI TTS guide](https://developers.openai.com/api/docs/guides/text-to-speech) documents `/v1/audio/speech`, chunked streaming, raw 24 kHz PCM, built-in voices, and the required AI-voice disclosure.
2. [OpenAI GPT-4o mini TTS model](https://developers.openai.com/api/docs/models/gpt-4o-mini-tts) lists the speech endpoint and current token pricing.
3. [OpenAI TTS-1 model](https://developers.openai.com/api/docs/models/tts-1) describes the lower-latency legacy model and $15 per million characters.
4. [Gemini speech generation](https://ai.google.dev/gemini-api/docs/speech-generation) documents 3.1 Flash TTS streaming through Interactions and raw 24 kHz PCM output.
5. [Gemini API pricing](https://ai.google.dev/gemini-api/docs/pricing) lists 3.1 Flash TTS free and paid token rates; free-tier inputs and outputs may be used to improve Google products.
6. [Gemini API terms](https://ai.google.dev/gemini-api/terms) state business-purpose use, generated-content ownership, regional restrictions, and paid/unpaid data-use differences.
7. [Groq TTS guide](https://console.groq.com/docs/text-to-speech) documents the OpenAI-compatible speech route and WAV-only output.
8. [Groq Orpheus guide](https://console.groq.com/docs/text-to-speech/orpheus) lists the English model, stock voices, 200-character limit, and $22 per million characters.
9. [Cartesia bytes API](https://docs.cartesia.ai/api-reference/tts/bytes) documents streaming HTTP audio bytes and raw PCM output.
10. [Cartesia SSE API](https://docs.cartesia.ai/api-reference/tts/sse) documents base64 audio events plus word and phoneme timestamps.
11. [Cartesia WebSocket API](https://docs.cartesia.ai/api-reference/tts/websocket) documents persistent multiplexed contexts, generation events, timestamps, and the cancel message.
12. [Cartesia endpoint comparison](https://docs.cartesia.ai/use-the-api/compare-tts-endpoints) explains why persistent WebSocket avoids per-turn TLS setup and when byte/SSE streaming is appropriate.
13. [Cartesia API conventions](https://docs.cartesia.ai/use-the-api/api-conventions) requires HTTPS and a dated `Cartesia-Version` header.
14. [Cartesia pricing](https://www.cartesia.ai/pricing) lists 20,000 monthly free credits, about 27 Sonic-3.6 minutes, two concurrent free requests, and commercial use beginning on the $5 Pro plan.
15. [Deepgram Flux quickstart](https://developers.deepgram.com/docs/flux-tts/quickstart) documents the `/v2/speak` streaming-first lifecycle and raw PCM formats.
16. [Deepgram Flux voices](https://developers.deepgram.com/docs/flux-tts/voices) identifies stock male voices including Miles, Kit, Cliff, and Colin.
17. [Deepgram Aura voices](https://developers.deepgram.com/docs/tts-models) identifies stock masculine voices including Arcas and Apollo.
18. [Deepgram streaming reference](https://developers.deepgram.com/reference/text-to-speech/speak-streaming) documents raw `linear16`, authentication, flush, and clear events.
    [Deepgram interruption handling](https://developers.deepgram.com/docs/flux-tts/interrupt-handling) documents immediate local playback stop, late-frame discard, `Interrupt`, and `SpeechInterrupted` reconciliation.
19. [Deepgram pricing](https://deepgram.com/pricing) lists Flux's temporary free period through 2026-09-12, then $0.045 per thousand characters, Aura-2 at $0.030, and Aura-1 at $0.015.
20. [Deepgram terms](https://deepgram.com/terms) permit API integration but restrict competitive benchmarking and describe the request-level training opt-out. Review this restriction before publishing a cross-provider result.
21. [Inworld streaming quickstart](https://docs.inworld.ai/quickstart-tts) documents the Basic credential, streamed WAV/PCM behavior, and official SDKs.
22. [Inworld streaming API](https://docs.inworld.ai/api-reference/ttsAPI/texttospeech/synthesize-speech-stream) documents `/tts/v1/voice:stream` and async timestamp transport.
23. [Inworld timestamp guide](https://docs.inworld.ai/tts/capabilities/timestamps) documents word, phoneme, and viseme timing, plus the latency tradeoff between sync and async delivery.
    [Inworld bidirectional WebSocket API](https://docs.inworld.ai/api-reference/ttsAPI/texttospeech/synthesize-speech-websocket) documents contexts, PCM, `send_text`, `flushCompleted`, and the flush-before-close behavior.
24. [Inworld pricing](https://inworld.ai/pricing) lists up to 70 free minutes, five concurrent requests, $15 per million characters for Flash, $25 for TTS-2, and a commercial license on On-Demand.
25. [ElevenLabs streaming API](https://elevenlabs.io/docs/api-reference/text-to-speech/stream) documents HTTP streaming and PCM formats.
26. [ElevenLabs latency guide](https://elevenlabs.io/docs/api-reference/reducing-latency) recommends Flash and streaming while clarifying that its advertised 75 ms is model inference rather than end-to-end latency.
27. [ElevenLabs subscription endpoint](https://elevenlabs.io/docs/api-reference/user/subscription/get/) exposes account character use and limits without generation.
28. [ElevenLabs API-key guide](https://elevenlabs.io/docs/overview/administration/workspaces/api-keys) distinguishes workspace quota from optional per-key credit caps.
29. [ElevenLabs pricing](https://elevenlabs.io/pricing/api) lists 10,000 free characters and $0.05 per thousand Flash/Turbo characters.
30. [ElevenLabs terms](https://elevenlabs.io/terms-of-use) limit free-tier output to non-commercial use; commercial use requires a paid subscription.

These are hosted APIs rather than redistributable model weights, so integration
is governed by each service's plan and terms. The ledger records current public
terms; it is not a substitute for a production legal review.
