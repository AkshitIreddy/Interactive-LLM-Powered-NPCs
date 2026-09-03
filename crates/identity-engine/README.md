# Identity engine

`npc-identity-engine` is the data-only actor continuity and conservative
identity layer for Interactive NPCs 2.0. It is intentionally independent of
capture, face-detector runtimes, the game process, and the UI.

## Integration boundary

For each advancing captured frame, an integration supplies `FrameActorsV1`
with detector-space boxes and optional `NormalizedEmbeddingV1` values. The
engine returns:

- sticky `TrackId` values and a `TrackEpoch` continuity token;
- lifecycle events for creation, loss, reacquisition, and retirement;
- the explicitly selected actor, retained longer through detector misses;
- conservative `Pending`, `Matched`, `Ambiguous`, `NoMatch`, `Explicit`, or
  `Offscreen` identity decisions; and
- a deterministic `enc_v1_*` encounter ID for every background actor.

Consumers must address asynchronous work by both `TrackId` and `TrackEpoch`.
A result produced before a loss/reacquisition boundary has a stale epoch and
must not be applied to the current frame.

## Embedding storage

The supported portable representation is the serde JSON shape of
`NormalizedEmbeddingV1` / `IdentityGalleryV1`. It records schema version,
provider, model ID, exact model revision, dimensions, preprocessing metadata,
crop bounds, and an optional source SHA-256. Tensors are finite L2-normalized
`f32` arrays. Gallery and tracker entry points validate deserialized data.

Language-specific object serialization such as Python pickle is not part of
the contract. The schema has no age, race, gender, or other demographic
inference fields and rejects unknown fields.

## Decision behavior

Recognition compares only embeddings from the exact same model space. It
requires a configurable number of supporting frames, exposes top-1/top-2
margin, and uses hysteresis before changing an already confirmed subject.
Close candidates remain ambiguous; weak candidates remain no-match. Explicit
assignment always takes precedence until cleared or the track retires.

