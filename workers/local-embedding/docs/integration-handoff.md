# Local embedding integration handoff

## Immutable identity

| Field | Exact value |
|---|---|
| Manifest | `packaging/model-packs/bge-small-en-v1.5-onnx-fp32.json` |
| Pack ID | `bge-small-en-v1.5-onnx-fp32` |
| Pack revision | `5c38ec7c405ec4b44b94cc5a9bb96e735b38267a.1` |
| Capability | `embedding`, generic |
| Source | `BAAI/bge-small-en-v1.5` |
| Source revision | `5c38ec7c405ec4b44b94cc5a9bb96e735b38267a` |
| ONNX artifact | `onnx/model.onnx`, 133,093,490 bytes |
| ONNX SHA-256 | `828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35` |
| Tokenizer artifact | `tokenizer.json`, 711,396 bytes |
| Tokenizer SHA-256 | `d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66` |
| Model-card license artifact | `README.md`, 94,783 bytes |
| Model-card SHA-256 | `ddb964361a55c6e5dfca6361615854b260c9c960205d04c7520151aaa1d75837` |
| Installed artifact bytes | 133,899,669 bytes |
| Peak transactional artifact bytes | 267,799,338 bytes |
| Runtime | `onnxruntime` 1.29.0 |
| ABI | `bge-bert-cls-f32-v1` |
| Initial backend | `cpu` only |
| Tensor | 384 x finite normalized f32, cosine |

The final manifest is frozen at raw SHA-256
`6ce53ad1ec837c7bfcaf541f07973a344a38540a3cc3e6832bbdebfa4d0e0ebc`.
The strict release-catalog generator observed canonical v2 SHA-256
`7266b0c735e7273826bab9e4ba58e3c8f5563dd871590263b3562176df6fc57c`
and normalized-core SHA-256
`f2be1c37d1fff448ca83574c83e6e26871aabeb06f69265f1ba3bde93b75394d`.
Catalog/TUF integration must bind those exact final bytes. The manifest keeps
portable planning RAM/load values and `expected_output_sha256` null because
machine and optimized CPU-kernel observations are not portable admission facts.
Hidden semantic/norm/dimension checks remain mandatory; the observed self-test
digest belongs only in a trusted device-bound signed envelope.

## Model Manager integration

1. Parse the manifest with the existing strict `ModelPackManifestV2` parser.
2. Require explicit player selection. The core installer remains model-free.
3. Reuse Model Manager's authenticated download journal, or port the lifecycle
   invariants from `pack_manager.py`: HTTPS only, bounded redirects, signed size,
   streamed SHA-256, `.part`, fsync, verify, atomic commit.
4. Expose explicit install, verify, repair, and remove commands. Remove should
   remain recoverable until normal cache cleanup.
5. Admit only a signed `QualifiedResourceEnvelopeV1` matching exact pack,
   revision, manifest digest, device fingerprint, runtime and backend.
6. For the first qualification use only `ResidencyModeV1::CpuResident` with
   `resident_vram_bytes = 0` and `p99_workspace_vram_bytes = 0`.
7. Unknown measurement, expired envelope, changed device/runtime, or fewer than
   20 samples blocks activation.

Expected measured placement fields:

```text
resident_ram_bytes
p99_total_ram_bytes
resident_vram_bytes = 0
p99_workspace_vram_bytes = 0
p99_load_millis
p99_reload_millis
p99_operation_millis
```

The worker's content-free report supplies process observations. The trusted
measurement harness supplies the device fingerprint, signed envelope, game and
desktop pressure snapshot, and p99 aggregation.

## Supervisor integration

Launch `worker.py` (or its frozen reviewed binary) with:

```text
--launch-nonce <secret supervisor nonce>
--worker-instance-id <opaque per-launch id>
--manifest <final manifest path>
```

The production process needs a supervisor-created current-user-only named pipe,
no remote clients, and a restricted token/job object. Preserve Worker Control
v1 fields. The worker never downloads and its audit hook denies sockets.

The `load` payload is exactly:

```json
{
  "lease_id": "opaque-model-manager-lease",
  "pack_id": "bge-small-en-v1.5-onnx-fp32",
  "revision": "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a.1",
  "artifact_root": "absolute exact installed revision directory",
  "manifest_sha256": "final manifest digest",
  "backend": "cpu",
  "cpu_threads": 2
}
```

`load` returns success only after rehash, ABI validation, and hidden self-test.
Never expose fixture backend selection in a production command.

## Runtime request and storage adapter

Send `infer` with `npc.embedding-request/v1`. Every item carries an opaque
input ID, exact text, its UTF-8 SHA-256, and optionally both storage `item_id`
and non-negative `generation`. Only `background` priority is accepted.

Map `npc.embedding-tensor/v1` as follows:

```text
model.model_id + model.revision -> EmbeddingInput.model_id
storage.item_id                 -> EmbeddingInput.item_id
storage.generation              -> EmbeddingInput.generation
items[].values                  -> EmbeddingInput.values
storage.*                       -> EmbeddingTensorMetadataV1
```

The character/memory stores should verify the source digest and raw f32le tensor
digest again at commit. Never compare tensors whose model or preprocessing
revision differs. Query-mode vectors are for query-to-passage retrieval; stored
memory/lore documents use passage mode.

## Scheduling and teardown

- Worker queue maximum: 64 requests / 256 items.
- Private inference batch: up to 32 items after a 4 ms coalescing window.
- Supervisor priority: lower than interactive LLM, STT, TTS, audio, subtitles,
  and current-frame lip-sync.
- Cancel advances the generation by exactly one and is the terminal barrier.
- Idle policy may keep the CPU session warm or unload based on measured reload
  cost; no GPU-resident state exists in revision 1.
- Resource pressure cancels queued embeddings before affecting speech.

## Qualification evidence and exact remaining integration gate

| Evidence | SHA-256 / result |
|---|---|
| Real Windows CPU report | `out/evidence/local-embedding-bge-cpu-2026-08-30.json` — `a2852d74ed08e93c5b02676cafeec21331ef5cfadfdfbf6a3301e2a3c5878463` |
| Python runtime inventory | `out/evidence/local-embedding-python-runtime-inventory-2026-08-30.json` — `c6ae5e84502a19d4304598c05f59875f0d678f330823ab1b2d4d6a558539d6af` |
| Healthy lifecycle verify | `out/evidence/local-embedding-lifecycle-verify-2026-08-30.json` — `fc74dc09e21bdc5bd326727aabb9ddf2af9933de9557d048f1651f08a1241155` |
| Recoverable remove | `out/evidence/local-embedding-lifecycle-remove-2026-08-30.json` — `05c1fa5e4bd88a7464ea6107bcdc8c3b0379f980dc8a73e0b0f07952bad9d648` |
| Exact-root cleanup | `out/evidence/local-embedding-cleanup-2026-08-30.json` — `eeb75a40e211affc1de32c15f5ca7aae7837c2f337f5ddc23dbebbddd0915ad5` |
| Terminal zero-allocation probe | `out/evidence/local-embedding-terminal-state-2026-08-30.json` — `7f8ed3284cd77c9fb3c5367c9e22e206878a841f8c4d69a409bf912c00b7cbd3` |

The report contains 24 load, 24 reload, 24 batch-one operation, 24 cancellation,
and 48 unload measurements. p99 load/reload/operation are 1,669/1,462/32 ms;
p99 cancel/unload are 28/55 ms. Resident/p99 RAM are
233,320,448/233,402,368 bytes. Resident/workspace VRAM are both zero. The quality
suite passed 6/6 top-1 and 6/6 top-3 with minimum expected margin
0.202652827746502.

Remaining work is intentionally outside this isolated lane:

1. a trusted resource-governor importer must bind the exact manifest/report,
   current device fingerprint, loadout telemetry, and signature into
   `QualifiedResourceEnvelopeV1`;
2. until then, expose `signed_measurement_envelope=false`,
   `admission_eligible=false`, no measured UI fields, and an empty qualified
   envelope list;
3. build a redistributable worker bundle from the exact 21-wheel runtime
   inventory (or an equivalently tested native sidecar), then integrate via
   reviewed Model Manager/runtime/Tauri commands and the full Windows matrix.
