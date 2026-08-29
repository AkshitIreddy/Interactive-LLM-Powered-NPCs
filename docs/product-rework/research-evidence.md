# Product rework research evidence

Research date: 2026-08-29  
Evidence set: 51 relevant sources from 23 organizations/domains. Primary and
official sources are preferred; older sources are included only where they
remain the governing API, license, implementation, or foundational paper.

## Windows capture, process, overlay, DPI, HDR, accessibility, and fonts

1. [Microsoft screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture), updated 2026-05-23. WGC frames carry `ContentSize`, D3D surfaces, and QPC-relative timestamps; frame pools must be recreated for size/device changes. FP16 is required for unclipped HDR capture.
2. [CreateForWindow](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow), updated 2024-04-11. Desktop apps can bind WGC to one exact HWND.
3. [Microsoft Win32 WGC sample](https://github.com/microsoft/Windows.UI.Composition-Win32-Samples/blob/master/cpp/ScreenCaptureforHWND/README.md), current. Minimized windows can be enumerated but do not advance capture.
4. [GraphicsCaptureSession.IsBorderRequired](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired), current. Borderless capture requires capability and consent; denial must leave a working bordered route.
5. [GraphicsCaptureSession.IsCursorCaptureEnabled](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.iscursorcaptureenabled), current. Cursor capture is a separate persisted/provenance choice.
6. [Microsoft Advanced Color](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range), current. HDR composition uses FP16 scRGB and display SDR-white metadata; FP16 has a measurable bandwidth cost.
7. [Default DPI awareness](https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process), updated 2025-07-14. Per-Monitor V2 belongs in the manifest before HWND creation.
8. [WM_DPICHANGED](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged), updated 2025-07-14. Apps must accept the suggested rectangle and re-layout on monitor transitions.
9. [Extended Window Styles](https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles), updated 2025-07-14. `NOACTIVATE`, `TOOLWINDOW`, `TOPMOST`, and layered/click-through styles have distinct behavior; accessible controls must remain in the main app.
10. [Window Features](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features), current. Layered `WS_EX_TRANSPARENT` windows pass pointer input through; passive and interactive overlay modes should be explicit.
11. [SetWindowDisplayAffinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity), updated 2024-02-22. `WDA_EXCLUDEFROMCAPTURE` can prevent app-owned overlay feedback but is not DRM/security.
12. [Process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags), updated 2025-07-14. `CREATE_NO_WINDOW` suppresses child consoles and must not be mixed with new-console/detached flags.
13. [Console allocation policy](https://learn.microsoft.com/en-us/windows/console/console-allocation-policy), updated 2025-08-05. Windows 11 24H2 manifest policy is an optional enhancement, not the compatibility baseline.
14. [Tauri create-app entry template](https://github.com/tauri-apps/create-tauri-app/blob/dev/templates/_base_/src-tauri/src/main.rs.lte), current. The Windows GUI subsystem attribute prevents the main console window.
15. [Rust Windows CommandExt](https://doc.rust-lang.org/std/os/windows/process/trait.CommandExt.html), Rust 1.97.1. Centralize flags, window state, stream redirection, and handle inheritance.
16. [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), updated 2025-07-14. One job can own/measure/terminate the child tree with kill-on-close.
17. [Tauri sidecars](https://v2.tauri.app/develop/sidecar/), current. Sidecar names, arguments, and capabilities should be explicit and allowlisted.
18. [Tauri WindowConfig](https://v2.tauri.app/reference/config/#windowconfig), current. Ordinary transparency/focusability/taskbar/topmost properties can be configuration; reserve native code for precise missing behaviors.
19. [Microsoft accessible text requirements](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/accessible-text-requirements), updated 2026. Default functional text needs 4.5:1 contrast, semantic roles, and testing with Magnifier/display/text scale.
20. [Xbox Accessibility Guideline 104](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/104), updated 2026-03-04. Subtitles identify speakers, avoid color-only meaning, scale to at least 200%, use mixed case/editorial line breaks, and normally stay within two lines.
21. [Xbox Accessibility Guideline 101](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/101), current 2026. Gameplay text must remain large, clear, and language-appropriate.
22. [Segoe UI](https://learn.microsoft.com/en-us/typography/font-list/segoe-ui) and [font redistribution FAQ](https://learn.microsoft.com/en-us/typography/fonts/font-faq), updated/current 2025. Segoe is a Windows fallback, not a generally redistributable app asset.
23. [SIL Open Font License FAQ](https://software.sil.org/oflt/), canonical. OFL fonts can ship when licenses, copyright, and Reserved Font Name duties are preserved.
24. [Google Noto distribution](https://github.com/notofonts/get-noto) and [usage guidance](https://github.com/notofonts/noto-docs/blob/main/docs/website/use.md), current. Noto is script-specific; test shaping and fallback boundaries rather than only missing glyphs.
25. [DirectWrite font fallback](https://learn.microsoft.com/en-us/windows/win32/api/dwrite_2/nn-dwrite_2-idwritefontfallback), updated 2024-02-22. Diagnostics should record the resolved family per subtitle run.
26. [W3C string metadata](https://www.w3.org/TR/string-meta/), current 2026. Text events require language/direction metadata and bidi isolation.

## Real-time voice, turn-taking, interruption, and observability

27. [OpenAI Realtime conversations](https://developers.openai.com/api/docs/guides/realtime-conversations), current. WebSocket clients own playback stop, heard-audio measurement, truncation, and cancellation; PTT provides deterministic boundaries.
28. [OpenAI Agents SDK voice agents](https://openai.github.io/openai-agents-js/guides/voice-agents/build/), current. VAD modes trade latency/naturalness; some model/voice settings apply only to a new session.
29. [Gemini Live capabilities](https://ai.google.dev/gemini-api/docs/live-api/capabilities), current. Interruption cancels generation/functions but queued playback remains client-owned.
30. [Azure Voice Live](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/voice-live-how-to), updated 2026-08. Playback cancellation, generation interruption, and history truncation are separate capabilities.
31. [LiveKit turn handling](https://docs.livekit.io/agents/logic/turns/), current. Manual mode suits PTT; interrupted history should contain only audio actually heard.
32. [Deepgram streaming latency](https://developers.deepgram.com/docs/measuring-streaming-latency), current. Measure end-of-turn and transcript latency separately and report distributions rather than one illustrative value.
33. [Deepgram voice-agent observability](https://developers.deepgram.com/docs/voice-agent-observability), current. Per-turn telemetry should include STT, token, TTS, total, acknowledgements, warnings/errors, and monotonic sequencing.
34. [ElevenLabs conversation flow](https://elevenlabs.io/docs/eleven-agents/customization/conversation-flow), current. Turn eagerness trades speed against interruption; timeouts and intentional pauses are distinct controls.
35. [LiveKit pipeline types](https://docs.livekit.io/agents/models/pipelines/), current. Speech-to-speech is lowest latency; cascaded STT→LLM→TTS is more auditable/configurable; model pipeline kind explicitly.

## Provider and model configuration

36. [OpenAI model catalog](https://platform.openai.com/docs/models), current. Models expose exact IDs, modalities, capability, limits, tools, and price metadata.
37. [Anthropic model overview](https://platform.claude.com/docs/en/models/overview), current. Compare latency, price, context/output, lifecycle, and exact platform IDs.
38. [Gemini models](https://ai.google.dev/gemini-api/docs/models), updated 2026-08. Stable, preview, live, TTS, embedding, and retired endpoints coexist; lifecycle must be visible.
39. [OpenRouter Models API](https://openrouter.ai/docs/api/api-reference/models/get-models), current. Provider metadata can include modalities, parameters, price, performance sorting, regional/ZDR filters, and expiry; distinguish reported from locally measured values.
40. [Ollama tags API](https://docs.ollama.com/api/tags), current. Local inventory should expose digest, bytes, format, family, parameter size, and quantization.
41. [LM Studio model loading](https://lmstudio.ai/docs/cli/local-models/load), current. Estimate memory before load; download and readiness are different states.
42. [LiteLLM model management](https://docs.litellm.ai/docs/proxy/model_management), current. Named credentials/tests/cost metadata work best with one canonical source of truth.
43. [Hugging Face model cards](https://huggingface.co/docs/hub/en/model-cards) and [gated models](https://huggingface.co/docs/hub/en/models-gated), current. Packs need license, provenance, limitations, digest, and recoverable access-required states.

## Actor tracking, identity, and subtitle practice

44. [Ultralytics tracking](https://docs.ultralytics.com/modes/track), current. Persistent IDs require sequential state; ByteTrack is a fast baseline while BoT-SORT/ReID adds resilience and cost.
45. [ByteTrack](https://github.com/FoundationVision/ByteTrack), ECCV 2022. Associating low-confidence detections improves occlusion continuity.
46. [BoT-SORT](https://github.com/NirAharon/BoT-SORT), 2022. Motion, appearance, and camera compensation reduce identity switches in crowded/moving scenes.
47. [HOTA](https://arxiv.org/abs/2009.07736), IJCV 2021. Evaluate detection/localization/association separately, plus switches, false matches, reacquisition, latency, CPU/GPU/VRAM.
48. [InsightFace licensing](https://github.com/deepinsight/insightface/blob/master/server/LICENSING.md), current through 2025-11. MIT source does not make public pretrained weights commercially redistributable.
49. [ONNX Runtime for Windows](https://onnxruntime.ai/docs/get-started/with-windows.html), current. WinML is the preferred Windows route; record the selected execution provider and retain CPU fallback.
50. [Game Accessibility Guidelines](https://gameaccessibilityguidelines.com/full-list/), current. Actor-following text is optional; clear customizable stable fallback remains necessary.
51. [Accessible Games Initiative criteria](https://accessiblegames.com/wp-content/uploads/2025/03/Accessible-Games-Initiative-Tags-and-Criteria-March-2025.pdf), March 2025. Subtitle size, color, background transparency, and presentation customization are first-class criteria.

## Synthesis and decisions

- Use one timestamp/identity chain from WGC frame through track, voice,
  subtitle, and delivered-turn commit.
- Prove target selection with positive frame deltas; “HWND found” is not
  “capture ready.”
- Use PMv2 coordinates and explicit HDR/SDR modes. Benchmark FP16 rather than
  silently enabling it.
- A passive overlay is not the accessible control surface. All controls and
  descriptive status stay in the main app.
- Fix all three console layers: GUI subsystem, child creation flags, and
  redirected bounded logs.
- Default first-run voice interaction to PTT. Cancellation coordinates provider
  generation, audio queue/device, subtitles, history, and visual response.
- Provider slots are typed by modality and pipeline kind. Persist exact IDs and
  metadata snapshots, not marketing labels.
- Track before identifying. Recognition is game-scoped assistance behind
  temporal consensus and manual correction.
- Model code and model weights have separate licenses. No downloadable artifact
  becomes a pack without an independently verified distribution license.
- Ship OFL subtitle fonts with notices and script-aware fallback tests; Segoe is
  a system fallback only.

