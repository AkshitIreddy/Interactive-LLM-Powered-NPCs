# BGE/ONNX local embedding evidence ledger

Research date: 2026-08-30  
Scope: low-interference English lore/memory retrieval on Windows while a game,
speech, and LLM routes remain interactive.

## Decision

Use `BAAI/bge-small-en-v1.5` at immutable commit
`5c38ec7c405ec4b44b94cc5a9bb96e735b38267a`, its upstream FP32 ONNX export,
official BERT tokenizer, CLS pooling and L2 normalization. Qualify CPU first.
Do not offer DirectML/CUDA in revision 1: embeddings are background work and an
unmeasured GPU placement would compete with the game. BGE Base and BGE-M3 remain
future quality/multilingual candidates, not silent fallbacks.

The research separates three evidence classes:

- exact upstream model/repository evidence;
- maintained implementation behavior;
- runtime, security, performance, and licensing evidence.

Mutable documentation informed the implementation, but install identity is
always the immutable model commit plus per-file size and SHA-256.

## Primary evidence set

All sources were opened and assessed on 2026-08-30. The set is deliberately
primary/official; search-result mirrors and third-party benchmark summaries are
not counted.

| # | Primary source | What it establishes | Implementation consequence |
|---:|---|---|---|
| 1 | [Immutable BGE Small model card](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/README.md) | Model identity, English scope, 384-d small variant, retrieval instruction guidance, MIT metadata, evaluation provenance | Exact source revision, English-only manifest, versioned query instruction, MIT notice |
| 2 | [Immutable repository tree](https://huggingface.co/BAAI/bge-small-en-v1.5/tree/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a) | Exact upstream file set and verified ONNX-support commit | Allowlist only ONNX/tokenizer artifacts; reject PyTorch pickle |
| 3 | [Immutable upstream ONNX file](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/onnx/model.onnx) | 133,093,490-byte ONNX payload and SHA-256 `828e1496...0cf35` | Manifest pins exact size/hash and rehashes before load |
| 4 | [Immutable tokenizer JSON](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/tokenizer.json) | Safe serialized tokenizer graph at exact model commit | Use `Tokenizer.from_file`; no remote code or pickle |
| 5 | [Immutable tokenizer configuration](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/tokenizer_config.json) | BERT tokenizer, lower-casing, 512 maximum, named special tokens | Enforce 512-token truncation and exact tokenizer revision |
| 6 | [Immutable pooling configuration](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/1_Pooling/config.json) | CLS-token pooling is enabled; mean/max pooling disabled | Select hidden state `[:, 0, :]`; reject output ABI drift |
| 7 | [Immutable sentence-transformer modules](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/modules.json) | Transformer then pooling module ordering | Keep tokenization/model/pooling as explicit stages |
| 8 | [Immutable BERT config](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/config.json) | BERT model family and 384 hidden size | Require BERT input names and 384 hidden output |
| 9 | [FlagEmbedding project](https://github.com/FlagOpen/FlagEmbedding) | BGE v1.5 release context and supported retrieval workflow | Prefer official behavior over generic feature-extraction defaults |
| 10 | [FlagEmbedding encoder inference guide](https://github.com/FlagOpen/FlagEmbedding/blob/master/examples/inference/embedder/README.md) | Query instruction, batch size, max lengths, normalization, and retrieval usage | Explicit mode and private bounded batching; no one-size-fits-all prefix |
| 11 | [FlagEmbedding encoder implementation](https://github.com/FlagOpen/FlagEmbedding/blob/master/FlagEmbedding/inference/embedder/encoder_only/base.py) | Default CLS pooling, L2 normalization, length-aware batching, adaptive OOM behavior | CLS+L2 is normative; scheduler bounds work before backend OOM |
| 12 | [BGE technical report](https://arxiv.org/abs/2309.07597) | Training/evaluation motivation for general embeddings and retrieval | Treat BGE as retrieval model, not identity/face embedding evidence |
| 13 | [Hugging Face immutable download guide](https://huggingface.co/docs/huggingface_hub/guides/download) | Revision-aware single-file/snapshot downloads and local cache behavior | Download exact commit files only; never resolve `main` during install |
| 14 | [Hugging Face cache internals](https://huggingface.co/docs/huggingface_hub/main/guides/manage-cache) | Commit-keyed file metadata and verification concepts | Store manifest/receipt identity and reverify cache contents |
| 15 | [Tokenizers batch quick tour](https://huggingface.co/docs/tokenizers/python/latest/quicktour.html) | `encode_batch`, right-padding behavior, and generated attention masks | Hidden batching uses one tokenizer batch and passes attention masks |
| 16 | [Tokenizers API](https://huggingface.co/docs/tokenizers/main/api/tokenizer) | Explicit truncation, padding, special-token, and batch APIs | Configure bounded truncation/padding rather than library-global defaults |
| 17 | [Transformers BERT contract](https://huggingface.co/docs/transformers/model_doc/bert) | `input_ids`, `attention_mask`, `token_type_ids`, CLS/special-token semantics | Input-name allowlist; optional token-type IDs; first token is intentional |
| 18 | [ONNX Runtime execution providers](https://onnxruntime.ai/docs/execution-providers/) | Ordered provider selection and CPU fallback behavior | Specify only CPU EP; no implicit GPU/fallback provider relabeling |
| 19 | [ONNX Runtime DirectML provider](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html) | DirectML constraints and required sequential/no-memory-pattern settings | Defer DirectML to a separately measured pack/backend revision |
| 20 | [ONNX Runtime CUDA provider](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html) | CUDA/cuDNN compatibility and provider memory options | No CUDA activation without separately pinned runtime/driver/resource evidence |
| 21 | [ONNX Runtime thread management](https://onnxruntime.ai/docs/performance/tune-performance/threading.html) | Intra/inter-op controls, sequential execution and thread spinning trade-offs | Default two intra-op threads, one inter-op thread, spinning disabled |
| 22 | [ONNX Runtime I/O binding](https://onnxruntime.ai/docs/performance/tune-performance/iobinding.html) | Device-copy cost and output allocation behavior | CPU v1 avoids GPU copy complexity; revisit binding only with GPU qualification |
| 23 | [ONNX Runtime graph optimization](https://onnxruntime.ai/docs/performance/model-optimizations/graph-optimizations.html) | Basic/extended/layout optimization and online/offline choices | Enable reviewed ORT optimizations and include load time in measurements |
| 24 | [ONNX Runtime memory guidance](https://onnxruntime.ai/docs/performance/tune-performance/memory.html) | Arena/shared-allocation trade-offs | Measure resident and p99 RAM; unload owns session/arena release |
| 25 | [ONNX Runtime profiling](https://onnxruntime.ai/docs/performance/tune-performance/profiling-tools.html) | Operator/thread latency trace support | Qualification may enable profiling; production default keeps content-free aggregates only |
| 26 | [ONNX Runtime logging/tracing](https://onnxruntime.ai/docs/performance/tune-performance/logging_tracing.html) | Runtime log severity and Windows ETW tracing | Production worker uses warning/error severity and never logs text/vectors |
| 27 | [ONNX Runtime Python API](https://onnxruntime.ai/docs/api/python/api_summary.html) | Session metadata, provider options, profiling clock, `RunOptions.terminate` | Validate live ABI and terminate active run on generation cancellation |
| 28 | [ONNX Runtime custom builds](https://onnxruntime.ai/docs/build/custom.html) | Reduced operators and pinned release-tag build guidance | Frozen sidecar may later use a reduced reviewed runtime, never `main` |
| 29 | [ONNX Runtime C deployment](https://onnxruntime.ai/docs/get-started/with-c.html) | Supported Windows CPU/GPU artifacts and session/tensor lifecycle | Native-sidecar replacement remains viable without changing tensor contract |
| 30 | [ONNX Runtime releases](https://github.com/microsoft/onnxruntime/releases) | 1.29.0 is the current release line observed on the research date | Runtime direct dependency pins 1.29.0 and exact wheel hash |
| 31 | [ONNX Runtime license](https://github.com/microsoft/onnxruntime/blob/v1.29.0/LICENSE) | MIT terms for runtime | Preserve copyright/license notice in packaged dependency inventory |
| 32 | [SPDX license list](https://spdx.org/licenses/) | Canonical `MIT` identifier and license URL semantics | Manifest uses SPDX `MIT`; notices preserve upstream attribution |

## Convergence, disagreement, and workload conditions

The upstream model card, pooling config, FlagEmbedding guide, and implementation
converge on CLS pooling plus normalized vectors. The main conditional behavior
is the query instruction: v1.5 is more useful without an instruction than older
BGE, but the upstream guide still recommends it for short-query-to-passage
retrieval. Therefore the worker requires an explicit `query`/`passage` mode and
versions that distinction; it never silently prefixes stored passages.

GPU guidance is not evidence that GPU is best for this workload. ONNX Runtime
documents DirectML/CUDA and I/O binding, while the product's actual constraint
is spare game VRAM and interaction priority. CPU-only is the evidence-backed
first placement. GPU remains blocked until a distinct p99 resource/latency/frame
impact measurement proves a benefit on the target machine.

An FP32 model was selected because the upstream repository publishes that exact
ONNX artifact. Dynamic INT8 quantization could reduce memory/latency, but it
would create a derived artifact with new quality, hash, license-notice, runtime,
and benchmark obligations. It is not mislabeled as an upstream pack revision.

## Saturation and excluded alternatives

The last runtime/security sources added packaging, profiling, and provider
constraints but did not change the model/pooling decision. The evidence set was
therefore considered saturated for this narrow lane.

- BGE Base/M3: potentially higher quality/multilingual reach, but materially
  larger and not justified for low-priority English memory indexing.
- community quantized exports: smaller, but not the official immutable artifact
  and require independent semantic-regression qualification.
- generic sentence-transformer defaults: may hide pooling/prefix/version drift.
- CUDA/DirectML: useful options, but unmeasured game interference means fail
  closed under the product's resource policy.
- remote embeddings: remain a separate provider route; they are not a fallback
  silently authorized by selecting this local pack.

## Completed local qualification and remaining trust gate

The authorized Windows CPU qualification completed under the repository AI lock
and the disposable runtime/model root was removed afterward. Frozen identities:

- manifest raw SHA-256:
  `6ce53ad1ec837c7bfcaf541f07973a344a38540a3cc3e6832bbdebfa4d0e0ebc`;
- tokenizer SHA-256:
  `d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66`;
- Windows-safe self-test fixture SHA-256:
  `3abdba8b0018a4f553a96cbd55662e1d803cc38bb5e24040337309e695e45f03`;
- observed self-test output digest:
  `3f7291e9c9fb96a1d3692f3ff5c384ae84fa6e15b0859129c2ab0df38eb3c70f`;
- qualification report SHA-256:
  `a2852d74ed08e93c5b02676cafeec21331ef5cfadfdfbf6a3301e2a3c5878463`.

Twenty-four load, reload, batch-one operation, and cancellation samples plus 48
unload samples established p99 load/reload/operation of 1,669/1,462/32 ms,
233,402,368 bytes p99 total RAM, and zero resident/workspace VRAM. The semantic
suite achieved 6/6 top-1 and 6/6 top-3. Exact lifecycle, runtime inventory, and
terminal-cleanup evidence is listed in the integration handoff.

The only deliberately pending evidence is a trusted current-device fingerprint
and signed `QualifiedResourceEnvelopeV1`. The report is hash-bound but was not
self-signed by the lane. Catalog/UI state must therefore remain unqualified and
non-activatable; measured UI fields and qualified-envelope lists stay empty.
