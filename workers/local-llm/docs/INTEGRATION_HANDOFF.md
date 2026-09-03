# Exact integration handoff

This lane intentionally does not edit shared catalog, model-manager, Tauri, or
runtime-host files. The following is the exact handoff for those owners.

## Identities

- Manifest: `packaging/model-packs/qwen3-4b-instruct-2507-q4-k-m.json`
- Current v2 manifest SHA-256:
  `b17aa18054478eb8394d2d98f38e3411340bd0673a54b278c7ed124f510c0ed9`
- Pack ID: `qwen3-4b-instruct-2507-q4-k-m`
- Revision: `Qwen3-4B-Instruct-2507`
- Capability/scope: `language_model` / `generic`
- GGUF: 2,497,281,120 bytes,
  `3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597`
- Runtime ABI: `npc-llama-server-openai-v1+b10689+qwen3`
- Runtime revision:
  `b10689@57291f2644af8c9df0dd8d44395881c5bdcf0ecd`
- Runtime trust record: `workers/local-llm/runtime-bundle.b10689.json`
- Worker entrypoint: `python -m npc_local_llm.worker`

## Model manager

1. Add the manifest digest and artifact pins to a threshold-signed catalog entry
   on the `optional-local` channel. Never treat the checked-in JSON alone as a
   production trust root.
2. Preserve the existing Qwen candidate's `source_project`, revision, Apache-2.0
   license, generic language-model capability, and explicit-download requirement.
3. Treat the b10689 CPU/Vulkan runtime as a separate MIT-licensed signed runtime
   unit. The selected variant must be recorded in the model lease.
4. The install/repair/remove implementation may reuse the core model-manager's
   stronger Windows safe-open and attested lifecycle. The Python lifecycle is a
   conformance/reference implementation for local review, not authority to skip
   TUF/catalog signatures.
5. Issue a load lease only after full artifact verification and an attested
   `qwen3_structured_dialogue_v1` self-test bound to the installed tree,
   transaction, runtime ABI/backend, and cancellation generation.

## Resource governor

Do not copy manifest hardware hints into admission. Produce a signed
`MeasuredResourceEnvelopePayloadV1` with:

- exact `PackRevision`, computed manifest SHA-256, `LanguageModel` capability,
  device fingerprint, runtime `llama.cpp`, b10689 revision, and backend `vulkan`
  or `cpu`;
- at least 20 samples and a monotonic sequence;
- `GpuResident` placement for Vulkan: resident/p99 total RAM, resident VRAM,
  p99 workspace VRAM, p99 load time, **p99 reload time**, and p99 operation
  time (`p99_reload_millis` is required by the frozen envelope shape);
- `CpuResident` placement for CPU, plus `CpuResidentGpuCold` only if actually
  measured;
- current validity interval and trusted signatures.

Evaluate it as part of the complete selected STT + LLM + TTS + lip-sync + game
loadout. GPU layers and CPU threads in the worker load payload must come from the
admitted placement; there is no automatic “all GPU” fallback.

The 2026-08-30 local-review run supplies one raw reload sample (8,572.354 ms),
not an admissible p99. The aggregator must leave the qualified field absent/null
until the minimum signed sample count is met. That run is bound to the
pre-migration manifest SHA-256
`4cc58867ed65a53789d49820abaa23d9a25f03d64fdef4d86996e9497ffb60a0`;
it must not be rebound to the current v2 digest even though artifact/runtime
identities did not change.

## Direct CUDA/Vulkan benchmark handoff

The benchmark-only CUDA trust record is
`workers/local-llm/benchmark/runtime-bundle.cuda12.4.b10689.json`, SHA-256
`df21c1773e6409884961be69ccc5dbd6547178835bf06d35b3ecce7a0161af27`.
It pins the official b10689 CUDA 12.4 program ZIP and matching cudart ZIP to
641,985,859 total download bytes and 1,158,588,189 expanded bytes. The
benchmark contract is `workers/local-llm/benchmark/benchmark-config.json`,
SHA-256
`a19f45f2a45115367e258b6e6d5a414dde37d1750b2a4ab331aff87248e2ae00`.

The direct harness emits 20 Vulkan plus 20 CUDA raw observations after two
warmups/backend in alternating ABBA/BAAB order. It explicitly binds the current
manifest/GGUF/fixture/runtime identities and uses identical normalized b10689
argv for context, batch, ubatch, KV, Flash Attention, GPU layers, thread counts,
parallel slots, cache policy, Jinja and reasoning policy. It measures load,
reload, operation, TTFT, throughput, inter-token latency, RAM and VRAM, and
performs structured output, cancellation/recovery, unload/reload and VRAM-return
checks for every observation.

Its report is intentionally unsigned and has
`admissible_for_resource_governor: false`. The aggregate manager must verify the
raw report and then construct/sign the existing qualified envelope; it must not
copy the local report's own digest into a signature field. `load_millis`,
`reload_millis`, and `operation_millis` each have exactly 20 samples/backend, so
the aggregator can truthfully populate p99 load/reload/operation only after its
normal device/runtime/manifest binding and signer threshold checks.

The separate game matrix has only five repeats/backend/state and explicitly
forbids p99. The LM Studio control is b10662/CUDA 12.8, has ten raw samples, and
also forbids p99/admission. Neither may be merged into the b10689 direct sample
sets.

## Worker supervisor wire

Launch with a random `NPC_WORKER_LAUNCH_NONCE` environment value. Do not put the
nonce on the command line. Pass the manifest and runtime-bundle paths as fixed,
application-owned arguments. The first framed request is `handshake`.

A `load` payload is:

```json
{
  "model_id": "qwen3-4b-instruct-2507-q4-k-m",
  "lease_id": "opaque-model-manager-lease",
  "verified_model_path": "C:\\...\\model\\Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
  "verified_model_sha256": "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597",
  "runtime_executable_path": "C:\\...\\llama-server.exe",
  "runtime_abi": "npc-llama-server-openai-v1+b10689+qwen3",
  "runtime_backend": "vulkan",
  "context_tokens": 8192,
  "cpu_threads": 8,
  "gpu_layers": 36,
  "process_memory_limit_bytes": 6442450944
}
```

The worker performs a complete GGUF hash again before starting the child. This
cost belongs in the measured load envelope. A future authenticated file-identity
lease can replace re-hashing only if it binds the live file handle, size, digest,
install transaction, and catalog identity at the same trust strength.

An `infer` payload accepts either `prompt` or bounded `messages`, plus
`max_tokens`, `temperature`, `top_p`, optional `seed`, and optional
`response_json_schema`. Structured token events carry
`content_kind: structured_json_fragment`; they must go through runtime-core's
structured buffer and must never be sent directly to TTS. The terminal
`llm_result` contains `text`, parsed `structured_response`, finish reason, and
usage. Existing strict `npc_response.v1` validation remains authoritative.

`cancel` is the only operation that advances generation. The supervisor must
discard every older event. If the HTTP request does not drain after response
closure, the worker kills llama-server and reports `runtime_preserved: false`;
the next turn must reload instead of assuming stale KV state is safe.

## Tauri and UI

- Keep every hosted/API loadout as the default and first recommendation.
- Surface this pack under an explicit **Optional local LLM** profile choice with
  exact download bytes and separate runtime choice.
- Wire install, verify, repair, remove, self-test, and measured-admission states;
  never expose an enabled Activate action from a planning estimate.
- Show full-loadout RAM/VRAM fit and game reserve before installation and again
  before activation.
- Preserve the per-turn immutable loadout snapshot so changing profiles cannot
  redirect an in-flight turn.
- Manual fallback is explicit. Do not silently switch from a selected local
  route to cloud or from cloud to local.

## Packaging and licensing

The application installer contains neither GGUF nor runtime ZIP. Model pack
installation preserves the pinned Qwen Apache license and conversion provenance.
Runtime installation preserves the pinned llama.cpp MIT license. Notices and
receipts survive repair and are deleted only with the exact unreferenced unit.
