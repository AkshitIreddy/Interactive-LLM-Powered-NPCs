# Current-frame mouth worker

This directory contains the C++20 product worker and reference proof for the
generic, non-injecting mouth-residual boundary. It never modifies a game frame,
selects no actor, and has no per-game rig or game-process dependency. A new
atlas compositor primitive accepts two already-authorized in-memory canonical
mouth patches, but no artifact loader or atlas registry is enabled in the
product route yet.

## What is real

- A deterministic TTS-viseme-to-eight-coefficient mapping.
- A causal PCM energy/zero-crossing fallback that drives mouth motion without
  pretending to perform phoneme recognition.
- Exact cancellation generation, actor ID, track ID, track epoch, frame
  sequence, device generation, geometry epoch, capture timestamp, audio clock,
  ROI, and deadline binding.
- A model-independent landmark adapter: qualified packs map native indices to
  left/right mouth corners and upper/lower lip centers. No pack/model is
  downloaded, embedded, or assumed by the compositor.
- An optional admitted OpenSeeFace MNV3/LM1 producer that rehashes the exact
  Model Manager-authorized models, ONNX Runtime DLLs, licenses, notices,
  version, and revision immediately before loading. It uses the pinned ONNX
  Runtime 1.22.1 CPU C API and never links or bundles those optional files.
- A queue of exactly one item: newer work replaces older unprocessed work.
- Hard fail-open gates for stale frames, expired leases, deadline misses,
  audio/video skew, actor or frame mismatch, occlusion, pose, confidence, ROI
  containment, and residual size.
- A CPU reference compositor that samples only the exact current source frame,
  performs a bounded geometric mouth warp, and emits a feathered premultiplied
  BGRA8 residual. Source pixels are immutable and bypass means no residual.
- A deterministic atlas compositor primitive that validates premultiplied
  enrollment patches, interpolates two states, rotates them to the current
  semantic mouth-corner axis, binds the result to the exact source frame and
  track, and changes no pixels outside the bounded mouth rectangle.
- A Windows GUI-subsystem `npc-mouth-worker.exe` with a bounded authenticated
  named-pipe protocol. The control process starts it suspended, assigns it to
  the parent kill-on-close job, then resumes it and attests its PID, creation
  time and fixed executable name.
- A production D3D11 path that imports the broker-duplicated exact-source NT
  handle, obeys the keyed-mutex handoff, copies the current frame to CPU for the
  bounded warp, and returns a distinct worker-owned residual texture lease.
- One-shot source/residual ownership acknowledgements, unpredictable session
  and lease nonces, generation cancellation, 80 ms residual expiry, and
  bounded retained-resource pressure.
- Deterministic tests, a timing benchmark, and a synthetic PPM proof generator.

This is deliberately not a photorealistic neural lip-sync model. Its purpose is
to make the safety, timing, identity, and compositor contract executable before
a qualified model or character atlas is attached.

## What is not claimed

The executable, authenticated caller, exact WGC/D3D lease, worker-death PCM
regression and broker presentation proof are implemented. That does **not** make
this a neural lip-sync model or establish arbitrary-game visual quality. The
worker can consume a qualified typed 66-point packet or run its optional
admitted OpenSeeFace producer over the exact leased current frame. It still
does not recognize or select the actor and cannot invent a trusted track from
manual selection. Model Manager must prove the exact installed content tree,
signed current-device envelope, live target PID, and whole-loadout fit. The
ordinary selected-turn route additionally requires the native identity
engine's current qualified actor lock. Today that identity authority remains
unqualified, so the wired coordinator emits an unavailable/fail-open receipt
without calling the worker. Audio and subtitles remain independent and
continue.

The atlas primitive does not yet load the project-owned `.npz` proof, map a TTS
provider's viseme IDs to atlas states, adapt skin color/lighting, or prove live
captured-game warping. It remains unavailable in the app until a versioned,
validated artifact loader and those visual/product gates exist.

## Build and run

From a Visual Studio developer shell:

```powershell
cmake -S native/mouth-worker -B out/build/native-mouth-worker-product -G "Visual Studio 17 2022" -A x64
cmake --build out/build/native-mouth-worker-product --config Release --parallel 4
ctest --test-dir out/build/native-mouth-worker-product -C Release --output-on-failure
./out/build/native-mouth-worker-product/Release/npc_mouth_worker_benchmark.exe
./out/build/native-mouth-worker-product/Release/npc_mouth_worker_synthetic_proof.exe ./out/mouth-worker-proof
```

The synthetic proof writes the untouched current frame and the same frame with
the independently composited residual. It also prints actor/track/frame binding
and different deterministic digests for the two images.

On the 2026-09-02 native-Windows RelWithDebInfo run, the optimized direct
reference atlas path validated, bilinearly warped and interpolated 206x143
canonical states into a 173x71 current mouth residual in 1.168 ms mean / 1.388
ms p95 over 250 iterations. This meets the proposed compositor-only <=2 ms p95
gate, but excludes audio classification, capture and presentation; the future
D3D11 shader and full product path still require separate measurements.

## Integration boundary

The media broker should remain the sole GPU-resource and presentation authority:

1. Broker leases one exact source texture to the attested worker PID.
2. Worker returns a distinct mouth-only residual lease plus `ResidualProposal`.
3. Broker verifies the handle, adapter, format, synchronization, lease, actor,
   epochs, exact frame and timestamp, confidence, containment, and deadline.
4. Any mismatch hides the overlay and leaves the live game untouched.

The Windows media broker implements that authority through additive visual
commands 14/15 while preserving playback commands 12/13. Command 29 supplies
only a read-only, post-WASAPI-release eight-bin RMS/peak timing envelope; no raw
PCM or fabricated viseme crosses into the product coordinator. The Tauri native
control plane owns the worker supervisor and never exposes handles, installed
pack paths, or launch credentials to the WebView. The native product smoke
proves exact source import, admitted-provider configuration, residual
presentation, handle-value reuse safety, and that an optional worker crash
during live PCM neither sends global cancel nor prevents endpoint drain.
