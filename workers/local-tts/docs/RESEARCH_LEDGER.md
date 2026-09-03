# Kokoro local TTS research ledger

Research was last reviewed on 2026-08-30. Only primary upstream documentation,
source, release metadata, papers, and platform documentation were used for the
implementation decision. URLs that carry a source commit or release tag are
preferred where the upstream supports them.

## Decision

The smallest practical candidate for this product is Kokoro-82M v1.0 INT8 using
the sherpa-onnx 1.13.6 C API on Windows x64 CPU. It avoids GPU contention with
the game and lip-sync worker, supports callback-driven incremental PCM and
cancellation, and uses fixed voice embeddings. This is a qualification
candidate, not a latency claim: the INT8 path must still be compared with FP32
on the target PC because quantization does not guarantee a faster kernel on
every CPU.

Only 28 English voices are public: the exact upstream v1.0 speaker order for
af_alloy through bm_lewis. The archive contains additional languages and
embeddings, but they stay inaccessible until language preprocessing, quality,
and attribution are separately qualified. Voice cloning, reference audio, and
arbitrary embeddings are prohibited.

The runtime is not accurately described as Apache-only. sherpa-onnx 1.x TTS
builds piper phonemization with eSpeak-NG; eSpeak-NG is GPL-3.0-or-later. The
manifest therefore discloses Kokoro Apache-2.0, Kokoro dataset attribution,
sherpa-onnx Apache-2.0, piper-phonemize MIT, eSpeak-NG GPL-3.0-or-later, and
ONNX Runtime MIT. Direct upstream retrieval is retained, and any project mirror
is blocked on legal review of notices and corresponding-source obligations.
This is an engineering license inventory, not legal advice.

The C API streams samples and progress. It does not return word, phoneme, or
viseme timestamps, so the worker exposes only the exact PCM sample clock and
process receipt timing. Lip-sync remains a separate audio-driven system.

## Pinned artifacts

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| sherpa-onnx-v1.13.6-win-x64-shared-MD-Release-lib.tar.bz2 | 7,215,482 | dca033829d3a7e74c127fc0d349a12257fb890fe5038a381ab1706e4b35cf0fa |
| kokoro-int8-multi-lang-v1_0.tar.bz2 | 131,839,838 | 75654a84864be26f345f020f4070c2c019e96dd1b7f9bf6e2ffd59efac6aa5a3 |
| model.int8.onnx | 114,298,054 | 77ef4f0513401d508ed7831f8504c7042df58bc75e004ec9666894590f999b1d |
| voices.bin | 27,678,720 | 8a77c0d397026208d22211f37670b5b3b11e03f190756b25a1d24041fced82a9 |
| lexicon-us-en.txt | 5,956,885 | 7daaab53a181be9885b853a8582bf1838186317e5dadacbcef9c426d6fa0da14 |
| tokens.txt | 687 | 6ebb6bb288f20f3ae8d004d3c2ca27697da27c037d75e81a60e2a6a663f95425 |
| espeak-ng-data/phondata | 550,424 | 4e0288957874029a8c3c9f41a8f517ad4bf18127046decbdd4b9d1d6807ce3a3 |
| model LICENSE | 11,358 | cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30 |

The release archive identities came from official GitHub release metadata. The
critical model identities came from immutable Hugging Face LFS metadata or
immutable raw content. Payloads were not downloaded during this fixture lane.

## Primary-source evidence

### Model, architecture, voices, and quality

1. [Kokoro-82M model card](https://huggingface.co/hexgrad/Kokoro-82M)
2. [Immutable Kokoro v1.0 model card](https://huggingface.co/hexgrad/Kokoro-82M/blob/f3ff3571791e39611d31c381e3a41a3af07b4987/README.md)
3. [Immutable upstream voice catalog](https://huggingface.co/hexgrad/Kokoro-82M/blob/f3ff3571791e39611d31c381e3a41a3af07b4987/VOICES.md)
4. [Immutable upstream repository tree](https://huggingface.co/hexgrad/Kokoro-82M/tree/f3ff3571791e39611d31c381e3a41a3af07b4987)
5. [Hugging Face model metadata API](https://huggingface.co/api/models/hexgrad/Kokoro-82M)
6. [Official Kokoro implementation](https://github.com/hexgrad/kokoro)
7. [StyleTTS 2 paper](https://arxiv.org/abs/2306.07691)
8. [iSTFTNet paper](https://arxiv.org/abs/2203.02395)

### sherpa-onnx model and streaming interfaces

9. [sherpa-onnx Kokoro documentation](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html)
10. [sherpa-onnx C API TTS documentation](https://k2-fsa.github.io/sherpa/onnx/c-api/tts.html)
11. [Pinned 1.13.6 C API header](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/sherpa-onnx/c-api/c-api.h)
12. [Pinned C streaming TTS example](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/c-api-examples/offline-tts-c-api.c)
13. [Pinned C++ streaming TTS example](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/cxx-api-examples/offline-tts.cc)
14. [Pinned Python TTS example](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/python-api-examples/offline-tts.py)
15. [Kokoro voices.bin generator](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/scripts/kokoro/generate_voices_bin.py)
16. [Kokoro sample generator](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/scripts/kokoro/generate_samples.py)
17. [Kokoro export workflow](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/.github/workflows/export-kokoro.yaml)
18. [C API implementation and object lifetime](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/sherpa-onnx/c-api/c-api.cc)

### Windows runtime and release identity

19. [Windows installation documentation](https://k2-fsa.github.io/sherpa/onnx/install/windows.html)
20. [Windows prebuilt C API packages](https://k2-fsa.github.io/sherpa/onnx/c-api/windows.html)
21. [Windows CPU source build documentation](https://k2-fsa.github.io/sherpa/onnx/install/windows/build-with-cpu.html)
22. [sherpa-onnx v1.13.6 release](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.6)
23. [Official TTS model release assets](https://github.com/k2-fsa/sherpa-onnx/releases/tag/tts-models)
24. [Pinned sherpa-onnx change log](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/CHANGELOG.md)
25. [Pinned wheel packaging and DLL names](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/setup.py)
26. [Pinned Windows ONNX Runtime integration](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/cmake/onnxruntime-win-x64.cmake)
27. [Windows build-path discussion](https://github.com/k2-fsa/sherpa-onnx/issues/3336)
28. [Native object-lifetime failure report](https://github.com/k2-fsa/sherpa-onnx/issues/2347)

### License closure

29. [sherpa-onnx Apache-2.0 license](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/LICENSE)
30. [Pinned sherpa TTS build switches](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/CMakeLists.txt)
31. [Pinned eSpeak integration](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.6/cmake/espeak-ng-for-piper.cmake)
32. [eSpeak-NG GPL-3.0-or-later license](https://github.com/espeak-ng/espeak-ng/blob/1.52.0/COPYING)
33. [piper-phonemize MIT license](https://github.com/rhasspy/piper-phonemize/blob/master/LICENSE.md)
34. [sherpa issue tracking removal of GPL eSpeak dependency](https://github.com/k2-fsa/sherpa-onnx/issues/3731)
35. [ONNX Runtime MIT license](https://github.com/microsoft/onnxruntime/blob/v1.27.1/LICENSE)

### Secure lifecycle and IPC

36. [Python tarfile extraction filters](https://docs.python.org/3/library/tarfile.html#extraction-filters)
37. [Microsoft named-pipe security and access rights](https://learn.microsoft.com/windows/win32/ipc/named-pipe-security-and-access-rights)
38. [Microsoft pipe client impersonation guidance](https://learn.microsoft.com/windows/win32/ipc/impersonating-a-named-pipe-client)
39. [Microsoft job objects](https://learn.microsoft.com/windows/win32/procthread/job-objects)
40. [Microsoft process creation flags](https://learn.microsoft.com/windows/win32/procthread/process-creation-flags)

## Rejected or deferred alternatives

- SadTalker and talking-head video generators solve rendered portrait video,
  not low-latency game-agnostic TTS. They belong in the lip-sync research lane.
- GPU TTS was deferred because local lip-sync and the game need deterministic
  VRAM reserve. A CPU TTS candidate is easier to admit alongside those loads.
- Arbitrary Hugging Face Python stacks add a large dependency and packaging
  surface. The pinned native C ABI gives a smaller supervised boundary.
- Non-English voices were deferred because exposing an embedding is not enough:
  language-specific phonemization, quality, and attribution need separate
  qualification.
- Text-derived phoneme timing was rejected as dishonest for this worker. The
  final audio can drift from predicted duration, and the pinned C API does not
  provide alignment.

