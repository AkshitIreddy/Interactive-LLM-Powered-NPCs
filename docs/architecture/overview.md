# Interactive LLM Powered NPCs 2.0 architecture

Status: accepted target architecture with partially integrated vertical slice

Primary platform: Windows 10 22H2 and Windows 11 x64

Product boundary: external integration with single-player games; protected online and
anti-cheat contexts fail closed

## Reading this document

**Integrated** means a source path exists in the current application. It does not imply
installed-app, physical-device, visual-quality, performance, or commercial-game proof.
**Target** means the contract is accepted but one or more production joins or
qualification gates remain open. Fixtures and component tests are never product proof.

```text
┌─────────────────────────────────────────────────────────────┐
│ Tauri 2 Response Console                                    │
│ React/TypeScript · onboarding · configuration · diagnostics │
└────────────────────────────┬────────────────────────────────┘
                             │ bounded commands/status only
┌────────────────────────────▼────────────────────────────────┐
│ Rust control + runtime host                                │
│ sessions · routes · memory · policy · timing · cancellation │
└──────────────┬─────────────────────┬─────────────────────────┘
               │ named pipe          │ optional pack workers
               │ protobuf envelope   │ after measured admission
               │ + JSON payload      │
┌──────────────▼──────────┐  ┌───────▼─────────────────┐
│ C++/WinRT media broker │  │ Native/local workers    │
│ WASAPI · WGC · D3D11   │  │ LLM/STT/TTS/embed/     │
│ hotkeys · overlay      │  │ vision/lip-sync roles  │
└─────────────────────────┘  └─────────────────────────┘
```

Shared-memory PCM and shared D3D handles are used only by implemented and qualified
producer/consumer pairs. They are not a general worker entitlement.

## Component truth

### Response Console

Tauri hosts React/TypeScript in WebView2. The WebView may issue bounded commands and
receive redacted state. Continuous PCM, captured pixels, D3D textures, credential values,
and model tensors remain native-only.

The mounted product has five job-oriented workspaces: Home, Games & characters, Voice &
intelligence, Appearance, and Health & support, plus bounded content, character-override,
and character-mouth-pack flows. Older `Pages.tsx` and standalone `Onboarding.tsx`
specimens remain unreachable from `App.tsx`; their fixtures are not product state. Only
state backed by native commands, persistence, and receipts may be presented as live.

### Rust control and runtime host

The integrated runtime owns sessions, cancellation generations, provider-route snapshots,
prompt assembly, delivery receipts, local SQLite memory, structured timing, and fail-open
optional work. The process protocol uses a length-delimited protobuf envelope; current
business requests and responses are versioned JSON within that envelope. See
`process-model.md` for the exact boundary.

Hosted LLM execution is integrated for a bounded set of provider-specific routes,
including fixed-origin Mistral and OpenRouter routes. The normal selected STT bridge
currently accepts AssemblyAI `u3-rt-pro`. Fixed-origin Cartesia, Deepgram, Inworld,
ElevenLabs, and NVIDIA Magpie TTS construction exists. A production Groq Qwen → Cartesia
component chain reached bridge PCM in 492.964 ms, but a complete microphone-to-physical-
speaker selected turn is not qualified. NVIDIA embedding retrieval has an integrated
bridge. Catalog entries and provider traits must not be read as executable-route support.

`NpcEffectsV1` remains a validated proposal schema. The current ordinary simulation path
constructs fixture effects; it does not actuate a game. Malformed, late, or unsupported
proposals are neutral no-ops and cannot delay speech.

### Resource admission

`crates/runtime-core` contains a ResourceBroker and an opt-in TurnSupervisor admission
path using a host-supplied measured whole-turn plan. It can reject reserve shortfalls and
release leases on completion or cancellation. This is integrated core policy, not yet a
production runtime-host claim: the host does not yet receive a trusted, native-stamped
selected-game budget plus qualified whole-turn model envelope.

Model Manager separately implements signed pack lifecycle policy, measured whole-loadout
preflight, and RAM/VRAM/game-reserve reasoning. Both layers must be joined to native
telemetry before local execution is production-admitted.

### Media broker

The C++20/WinRT broker implements WASAPI transport, system hotkeys, Windows Graphics
Capture by HWND, D3D11 resources, DirectComposition presentation, native target geometry,
and Desktop Duplication fallback policy. It has no credential, prompt, lore, memory, or
network authority.

The native implementation still needs installed physical proof across display modes,
DPI/HDR, move/resize, device loss, one/multiple monitors, and real games. Its existence
does not qualify normal commercial-game capture.

### Optional local workers and packs

The base installer remains API-first and model-free. After an explicit user choice, the
pack architecture may support generic language-model, speech-recognition,
speech-synthesis, embedding, vision, and lip-sync roles. Every role requires immutable
provenance, license, signature, measured p99 resource/latency envelope, whole-loadout
admission, and a safe fallback. No pack is silently downloaded, activated, substituted,
or represented as available because a catalog candidate exists.

The supervised local LLM worker is an integrated route, but its current sample pack is
not admissible production evidence. Other role catalogs and workers remain candidates
until independently qualified.

### Profiles, game targeting, identity, and visuals

Profiles are data-only declarations. They never install or require a mod, hook, injected
DLL, script extender, game-memory reader, or native-rig adapter. A profile can be
data-complete without being capture-, identity-, subtitle-, or animation-qualified.

The rights-cleared Eclipse Harbor profile and project-owned moving review game are the
current synthetic safe targets. Authored
non-synthetic profiles deliberately run as console-isolated conversations with no game
interaction or visuals. Normal commercial-game capture and overlay remain unqualified.

Identity contracts, a native worker bridge, manual-picker states, sticky tracks, and
actor-lock epochs exist. A private 2-of-2 signed YuNet/LM1 catalog binds fresh measured
CPU evidence. Its isolated activation qualification proved a fresh authenticated hidden
worker, exact active inventory, and retry cleanup. Setup activation still cannot authorize
live rendering, and normal user-state installation/whole-loadout admission are not
qualified. The production actor-lock bus therefore starts unqualified, and the moving-
character visual route cannot be called integrated product behavior.

Generic screen-space lip-sync remains optional and fail-open. Keep the actor/frame/audio
clock, current-frame, mask, freshness, and cancellation contracts. The September 7
schema-3 Cyberpunk/Misty moderate-OH replay preserves the native admission digest and
containment while making rounded articulation more restrained. A photometric/pasted seam
remains, and its generated anatomy is not observed game anatomy. It is accepted for local
review only, not as natural-animation, installed-provider, live-game, or latency proof,
and must not be advertised or made a dependency of audio/subtitles.

## System invariants

1. The foreground game has resource priority.
2. No subsystem silently crosses the configured local/cloud boundary.
3. Optional vision, lip-sync, and effects cannot stop audio/subtitles.
4. Only delivered dialogue is committed as heard history.
5. Profile, model, and provider output is data, never executable code.
6. Cross-process messages are bounded, ordered, versioned, cancellable, and validated at
   both the envelope and business-payload layers.
7. Derived embeddings and indexes are rebuildable; authoritative text/state is
   transactional.
8. Rejected or stale visual work leaves the untouched game presentation visible within
   one displayed frame.
9. Capture/overlay fails closed under online, anti-cheat, protected-content, or target
   ambiguity. Injection and bypass are out of scope.
10. No demographic inference is part of the product flow.
11. Catalog presence, fixtures, or synthetic receipts do not establish availability.
12. Push, publication, and release remain separate approval-gated actions.

## Deployment units

| Unit | Base installer | Current qualification |
| --- | --- | --- |
| Tauri control application | Yes | Source integrated and headless browser-reviewed; installed/clean-machine qualification open |
| Rust runtime host | Yes | Source integrated; full ordinary live turn open |
| C++ media broker | Yes | Native source integrated; physical display/game matrix open |
| Deterministic fixtures | Yes, development/review only | Never production evidence |
| Optional local role packs/runtimes | No | Explicit install only; private YuNet provider-load lifecycle qualified in isolated state; normal installation/live authority/production qualification open |
| Built-in data-only profiles | Yes | Content/schema coverage; live capabilities qualify separately |
| Per-game mods, hooks, injected DLLs, rig adapters | No | Unsupported architecture |

The target NSIS install requires no Python, Node, Rust, CUDA toolkit, FFmpeg, notebook, or
pip from the user. That remains an acceptance condition until proven on clean Windows
machines.

## Degradation order

1. Drop stale optional vision or visual work.
2. Disable experimental screen-space lip-sync.
3. Neutralize optional effects/style proposals.
4. Fall back from semantic retrieval to local FTS/recent delivered context.
5. Continue conversation through audio and subtitles when target policy permits.
6. Offer typed input when STT fails or subtitles when TTS fails.
7. Surface a retryable LLM error.

Cross-provider fallback occurs only when the user pre-authorized the exact route and its
privacy/cost consequences. Local/cloud or device substitution is never implicit.

## Decision records

- `adr/0001-tauri-control-plane.md`
- `adr/0002-custom-rust-runtime.md`
- `adr/0003-ipc-and-process-isolation.md`
- `adr/0004-provider-abstractions.md`
- `adr/0005-local-model-runtime-and-packs.md`
- `adr/0006-sqlite-memory.md`
- `adr/0007-native-and-screen-space-lipsync.md`
- `adr/0008-capture-and-overlay.md`
- `adr/0009-security-and-update-boundaries.md`
