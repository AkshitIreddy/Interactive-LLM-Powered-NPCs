# Optional BGE local embedding worker

This directory is the isolated production lane for an optional local embedding
pack. It does not bundle model weights, does not silently install anything, and
is not connected to the product runtime yet.

The selected model is `BAAI/bge-small-en-v1.5` at immutable Hugging Face commit
`5c38ec7c405ec4b44b94cc5a9bb96e735b38267a`. The reviewed upstream FP32 ONNX
artifact is 133,093,490 bytes with SHA-256
`828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35`.
The model card declares MIT. A copy is downloaded only after the player chooses
the pack and the Model Manager verifies the complete manifest, license notice,
available storage, and resource admission.

## Why this model and backend

BGE Small English v1.5 is a 384-dimensional encoder suited to English
character/lore retrieval. The small variant is preferred over BGE Base/M3 for
this optional role because embeddings run below interactive speech and LLM work.
The initial qualified placement is CPU-only ONNX Runtime: it consumes zero game
VRAM by construction and lets the resource governor deprioritize, cancel, or
unload background indexing without disrupting speech.

The implementation preserves the upstream contract:

- official BERT tokenizer metadata, 512-token truncation, right padding;
- the official retrieval instruction only for query-to-passage searches;
- first-token (`CLS`) pooling;
- finite 384-element float32 tensors normalized to unit L2 norm;
- cosine/dot-product comparison only inside the exact model and preprocessing
  revision.

## Files and ownership

- `model_spec.py` validates immutable pack identity, strict requests, f32
  normalization, and storage metadata.
- `pack_manager.py` implements explicit install, verify, transactional repair,
  and recoverable remove. Downloads land in `.part` files and never enter the
  active tree before size and SHA-256 match.
- `backend.py` lazily imports the pinned ONNX/tokenizer runtime only after the
  Model Manager presents a verified lease. The inference process has no network.
- `scheduler.py` privately batches bounded background requests, enforces
  deadlines, and uses monotonic cancellation generations.
- `protocol.py`, `framing.py`, and `worker.py` implement Worker Control v1 over
  length-delimited JSON. The production supervisor supplies the authenticated
  named pipe; stdio remains the test/debug transport.
- `telemetry.py` reports content-free process RAM, CPU, queue, batch, load, and
  latency metrics. CPU placement reports VRAM zero as a backend invariant;
  unmeasured accelerator placement is `null`, never guessed.
- `embedding-tensor-v1.schema.json` is the versioned output boundary.
- `manifest_builder.py` and `build_manifest.py` refuse placeholder tokenizer
  hashes and deterministically emit the final catalog manifest after the gated
  evidence exists.
- `benchmark-plan.v1.json` is the gated real qualification plan.
- `qualify.py` keeps new-backend load and same-backend unload/load reload
  distributions distinct and emits the governor's required p99 fields.
- `runtime-requirements.windows-x86_64-cp312.v1.json` records the direct runtime
  wheel identities; packaging must add and license the full transitive lock.

## Supervisor lifecycle

```text
starting --handshake--> cold --warm--> warm --verified load+self-test--> loaded
loaded --infer(background)--> private bounded batch --> embedding_batch
any active generation --cancel(N+1)--> no old tensor or terminal event
loaded --unload--> warm --shutdown--> stopped
```

`load` requires the exact pack/revision, manifest SHA-256, Model Manager lease,
CPU backend, and artifact root ending in the exact pack/revision. It rehashes
the model and tokenizer, validates the ONNX input/output ABI, and runs a hidden
semantic self-test before reporting success. A corrupt or partially measured
pack is not activated.

## Storage compatibility

Each result includes:

- model provider, ID, immutable revision, pack revision, and 384 dimensions;
- preprocessing revision including `query` or `passage` mode;
- normalized JSON float values;
- optional `character-db/1.0.0` storage metadata containing item/generation,
  source-content SHA-256, raw f32 little-endian tensor SHA-256 and byte length.

The adapter maps directly to `EmbeddingTensorMetadataV1` and `EmbeddingInput`.
Pickle, executable object serialization, paths, URLs, model-generated item IDs,
and unversioned tensors are not accepted.

## Focused verification

Fixture-only tests do not import ONNX Runtime or touch the model/GPU:

```powershell
py -3.12 -m unittest discover -s workers/local-embedding/tests -v
```

The gated Windows CPU qualification completed on 2026-08-30 after API E2E and
the local-model queue released this lane. It used 24 load, 24 reload, 24
batch-one operation, 24 cancellation-barrier, and 48 unload samples. The
measured p99 values were 1,669 ms load, 1,462 ms reload, 32 ms batch-one
operation, 28 ms cancellation, and 55 ms unload. Resident RAM was 233,320,448
bytes, p99 total RAM was 233,402,368 bytes, and CPU placement used zero resident
and workspace VRAM. The six-query semantic suite achieved 6/6 top-1 and 6/6
top-3 retrieval with a 0.20265 minimum expected margin.

The exact report is
`out/evidence/local-embedding-bge-cpu-2026-08-30.json` (SHA-256
`a2852d74ed08e93c5b02676cafeec21331ef5cfadfdfbf6a3301e2a3c5878463`).
The pack and disposable Python runtime were removed after verification. This is
real local evidence, but it is deliberately **not** an admission credential:
the trusted resource-governor workflow must still bind a current device
fingerprint and signature. Until that exists, activation remains blocked.
