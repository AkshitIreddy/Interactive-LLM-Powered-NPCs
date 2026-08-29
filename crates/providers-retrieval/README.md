# Hosted retrieval provider contracts

This crate defines versioned, provider-neutral embedding and reranking contracts and an NVIDIA
NIM hosted adapter. It is a network boundary only; it does not own the SQLite memory schema,
retrieval policy, automatic fallbacks, credentials, or model catalog.

The adapter currently supports:

- bearer-authenticated text embeddings at `https://integrate.api.nvidia.com/v1/embeddings`;
- a text-reranking contract for the allowlisted
  `https://ai.api.nvidia.com/v1/retrieval/nvidia/reranking` endpoint;
- exact curated model IDs with no alias rewriting;
- explicitly curated, NVIDIA-hosted model-specific reranking paths;
- query-versus-passage embedding roles, bounded inputs, cancellation, deadlines, sanitized typed
  errors, ordered float vectors and ordered relevance scores;
- request-scoped non-cloneable credentials which are zeroized after transport dispatch.

Both query and passage content leave the PC. Provider retention depends on the current NVIDIA
account and terms. A failed call never falls back to another provider automatically.

The implementation follows NVIDIA's official embedding and reranking API contracts:

- <https://docs.api.nvidia.com/nim/reference/nvidia-llama-nemotron-embed-1b-v2-infer>
- <https://docs.nvidia.com/nim/nemo-retriever/text-embedding/latest/reference.html>
- <https://docs.nvidia.com/nim/nemo-retriever/text-reranking/latest/using-reranking.html>

Tests use a deterministic mock transport and make no live network calls.

Current qualification note (2026-08-28): separate authorized synthetic smoke evidence confirmed
`nvidia/nemotron-3-embed-1b` at the hosted embeddings endpoint (2048 finite dimensions). Both the
generic and model-specific documented hosted reranking URLs returned HTTP 404 during that smoke.
Reranking therefore remains contract/mock verified and must stay non-selectable until signed
curated metadata identifies a currently working endpoint and the route is requalified.
