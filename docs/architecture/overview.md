# Interactive LLM Powered NPCs 2.0 architecture

Status: accepted target architecture  
Primary platform: Windows 10 22H2 and Windows 11 x64  
Product boundary: single-player games; protected online/anti-cheat contexts are blocked

## Goals

The architecture optimizes for low time-to-first-audio beside a GPU-intensive game, safe failure isolation, ordinary-user installation, explicit hosted-provider choice, deterministic testing, and replaceable API adapters. It does not preserve v1 implementation compatibility.

```text
┌─────────────────────────────────────────────────────────────┐
│ Tauri 2 Response Console                                   │
│ React/TypeScript · onboarding · configuration · diagnostics │
└────────────────────────────┬────────────────────────────────┘
                             │ commands/channels (control only)
┌────────────────────────────▼────────────────────────────────┐
│ Rust NPC runtime                                            │
│ sessions · turn state · policy · providers · memory · QoS   │
└──────────────┬────────────────────┬───────────────────┬──────┘
               │ Protobuf/named pipe│                   │
┌──────────────▼──────────┐ ┌───────▼────────┐
│ C++/WinRT media broker │ │ Inference       │
│ WASAPI · WGC · D3D11   │ │ workers/packs   │
│ hotkeys · overlay      │ │ native/Python   │
└─────────────────────────┘ └────────────────┘
       shared-memory PCM / shared D3D handles where qualified
```

## Component responsibilities

### Response Console

Tauri 2 hosts a React/TypeScript UI using the Windows WebView2 Evergreen runtime. It owns onboarding, Home, Games, Characters, Conversation, Presence, Performance, Models, Diagnostics, Settings and Help. It may send bounded commands and receive ordered status channels, but continuous PCM, captured frames, D3D textures, raw API keys, or model tensors never enter the WebView.

The visible Response Spine is driven by measured runtime events:

`Listening → Transcribing → Identifying → Remembering → Responding → Voicing → Animating`

A stage can be skipped, degraded, failed, cancelled or completed; the UI never invents progress from timers.

### Rust runtime

A custom Tokio state machine is the source of truth for sessions and turns. It enforces deadlines, cancellation generations, provider policy, privacy/egress decisions, memory transactions, resource leases, degradation, structured logs and end-to-end timing. The bounded conversational workflow does not require LangChain, LangGraph, or a general-purpose agent framework.

The runtime exposes versioned interfaces:

- `EnvelopeV1` for process messages;
- `StreamingRecognizer`, `LanguageModelProvider` and `TtsSession` for capabilities and streams;
- `NpcEffectsV1` for independently validated non-spoken effects;
- `GameProfileV2` for data-only supported-game behavior;
- `ModelPackManifestV1` for optional immutable generic lip-sync packs.

### Media broker

A C++20/WinRT process owns Windows-native real-time media: event-driven WASAPI, system hotkeys, Windows Graphics Capture by HWND, D3D11 resources, DirectComposition presentation, per-monitor DPI/HDR transforms and qualified DXGI Desktop Duplication fallback. It uses shared memory for PCM and shared D3D handles only where the consuming backend has passed interop and lifetime tests.

The broker has no provider credentials, prompt content, memory database access or authority to make online requests.

### Inference workers

Conversation uses hosted LLM, STT, TTS, and retrieval adapters. Local inference workers are reserved for optional generic screen-space lip-sync packs selected by the user. A downloadable CPython 3.12 runtime may accompany a qualified lip-sync pack only when that candidate's strongest supported Windows runtime requires it. Workers receive typed, bounded requests; they do not read global configuration or credentials and cannot execute profile or model output.

### External game integration and profiles

Profiles are signed, data-only capability declarations containing lore, characters, prompts, detection, capture hints, safety rules, diagnostics, and troubleshooting. They never install or require a mod, script extender, hook, DLL, injected code, or per-game native-rig adapter.

The supported integration is deliberately game-agnostic:

1. select a capturable single-player game window with WGC or qualified DXGI fallback;
2. select/name the intended character manually, with optional read-only OCR/screen evidence only after independent qualification;
3. deliver conversation through audio and subtitles;
4. optionally add generic, reversible screen-space lip-sync after it passes the visual/resource gates.

Anti-cheat, protected content, online mode, or ambiguous shared executables fail closed. Audio/subtitles may continue only when policy permits and without capture or overlay. Community and built-in profiles use the same data-only boundary.

## System invariants

1. The foreground game has resource priority.
2. No subsystem silently crosses the user’s configured local/cloud boundary.
3. Optional visual/effect failures cannot stop audio conversation.
4. Only delivered dialogue is committed as heard history.
5. Profile/model/LLM content is data, never code.
6. Every cross-process stream is ordered, bounded, cancellable and versioned.
7. Derived embeddings and indexes are rebuildable; authoritative text/state is transactional.
8. Stale visual output restores untouched game presentation within one displayed frame.
9. Capture/overlay is disabled under online/anti-cheat ambiguity; injection and bypass are out of scope.
10. Public release actions require explicit user approval.

## Deployment units

| Unit | In base installer | Update strategy |
| --- | --- | --- |
| Tauri UI/runtime executable | Yes | Signed app update, inactive until release approval |
| C++ media broker | Yes | Same signed app bundle |
| Minimal deterministic simulation/fixtures | Yes | Same bundle |
| Optional generic lip-sync models/runtime | No | Explicit user-selected, TUF-protected pack install after qualification; never automatic |
| Built-in data-only profiles | Yes | Signed/profile-versioned bundle or catalog |
| Per-game mods/adapters/hooks | No | Not a supported distribution or integration path |

The NSIS installer does not require Python, Node, Rust, CUDA toolkit, FFmpeg, notebooks, or pip. Developer toolchains remain lockfile-pinned repository inputs, not end-user prerequisites.

## Degradation ladder

The resource and failure policy applies this order and reports the reason:

1. reduce continuous vision sampling;
2. disable experimental screen-space lip-sync;
3. neutralize optional emotion/voice-style effects;
4. use SQLite FTS/recent context without hosted semantic retrieval;
5. continue selected/offscreen character through audio/subtitles;
6. offer typed input if STT fails or subtitles if TTS fails;
7. surface a retryable LLM error.

Cross-provider fallback is allowed only if the user pre-authorized the exact route and its privacy/cost implications.

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
