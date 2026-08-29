# Worker Control Protocol v1

Worker Control v1 is a local, supervisor-to-worker protocol. JSON framing is the mandatory development/debug encoding. Production adapters may use the protobuf encoding in `worker-control-v1.proto`; both encodings have the same field names, operation names, limits, and state machine.

## Transport

Each frame is `u32be length || payload`. JSON payloads are UTF-8 objects. The default streams are stdin/stdout. Windows production uses byte-mode named pipes created by the Rust supervisor with a current-user SID ACL, a per-launch nonce, and no remote clients. A worker may open paths passed by the supervisor but must not create its own pipe security policy.

Limits are checked before allocation:

| Item | Limit |
|---|---:|
| Encoded frame | 1,048,576 bytes |
| Text field | 262,144 UTF-8 bytes |
| Decoded inline binary | 524,288 bytes |
| Batch entries | 256 |
| Embedding dimensions | 4,096 |
| Output events per request | 4,096 |

Continuous PCM and video frames are out-of-band. A production request uses a lease containing a shared-memory region or shared D3D handle, byte length, format, lease ID, and expiry. The worker must not retain or reopen an expired lease.

## Request envelope

```json
{
  "protocol_version": "1.0",
  "worker_instance_id": "runtime-issued-nonce-bound-id",
  "request_id": "req-0001",
  "sequence": 1,
  "generation": 0,
  "deadline_unix_ms": 0,
  "operation": "handshake",
  "payload": { "launch_nonce": "...", "supervisor": "npc-runtime" }
}
```

`deadline_unix_ms: 0` means no deadline. Production should always provide a bounded deadline to `load`, `warm`, and `infer`. The worker rejects duplicate request IDs, non-increasing sequence values, mismatched instance IDs after handshake, expired deadlines, and unsupported protocol versions.

## Events

Every output repeats the request identity and adds an event index. `accepted` is emitted before asynchronous inference. Zero or more `progress`/modality events follow, then a terminal `completed` or `error` event. Advancing the cancellation generation supersedes older inference requests; those jobs stop without writing a now-stale terminal event. The supervisor treats the successful `cancel` response as the terminal generation barrier.

```json
{
  "protocol_version": "1.0",
  "worker_instance_id": "worker-llamacpp-stub",
  "request_id": "req-7",
  "sequence": 7,
  "generation": 3,
  "event_index": 2,
  "event": "token",
  "terminal": false,
  "payload": { "text": "Welcome" }
}
```

Errors have stable codes and safe messages. `details` may contain bounded scalar diagnostics, never raw user/model content.

## Lifecycle

```text
starting --handshake--> cold --warm--> warm --load--> loaded --infer--> loaded
                                  |                    |
                                  +------unload--------+

any live state --shutdown--> stopped
any active infer --cancel(N+1)--> same lifecycle state, generation N+1
```

- `handshake`: authenticates the launch nonce, binds the instance ID, and returns descriptor, capabilities, limits, and resource estimates.
- `capabilities`: returns stable capabilities without loading a model.
- `health`: returns lifecycle state, active generation, loaded model, warm status, in-flight count, and sanitized counters.
- `warm`: initializes runtime-only resources. Repeating it succeeds without duplicating resources.
- `load`: activates a Model Manager lease. Repeating the same lease/model is idempotent; a different model replaces it only after active inference has drained or been cancelled.
- `unload`: cancels/drains active work and releases model-owned resources. Repeating it succeeds.
- `infer`: emits modality-specific events. It requires the current cancellation generation and a loaded model.
- `cancel`: is the only operation that advances generation. It cancels all older work. The new generation must be exactly current + 1; retrying the current generation is idempotent.
- `shutdown`: cancels work, unloads resources, emits a terminal response, and exits cleanly.

Requests with a lower generation receive `stale_generation`. Requests with a higher generation receive `generation_gap`, except valid `cancel`.

## Stable error codes

| Code | Meaning |
|---|---|
| `invalid_frame` | Framing, UTF-8, JSON, numeric, or root-shape failure; the process closes the input transport after responding. |
| `invalid_request` / `invalid_payload` | The envelope or modality payload violates a bounded contract. |
| `unsupported_protocol` / `unsupported_operation` | The supervisor and worker do not share the requested contract. |
| `authentication_failed` / `instance_mismatch` | Launch binding failed. Do not retry with guessed values. |
| `handshake_required` / `invalid_state` / `model_not_loaded` | The lifecycle prerequisite was not met. |
| `duplicate_request` / `out_of_order_sequence` | Replay or reordering was detected. |
| `deadline_exceeded` | Work arrived after its deadline. |
| `stale_generation` / `generation_gap` | Cancellation ordering was violated. |
| `worker_busy` | Concurrency or safe model-replacement policy rejected work; retry after drain/cancel. |
| `inline_result_too_large` | The result needs an out-of-band shared-memory lease. |
| `event_limit_exceeded` | A faulty adapter exceeded its bounded stream budget. |
| `generation_mismatch` | A generic lip-sync payload was not bound to its request envelope generation. |
| `stale_source_frame` / `stale_media_lease` | A current-frame residual cannot remain valid through its presentation deadline. |
| `inference_failed` / `internal_error` | Sanitized adapter failure. Consult local structured diagnostics by correlation ID. |

## Deterministic stub payloads

The stubs intentionally consume fixture-like metadata rather than pretending to process media:

- LLM: `messages` or `prompt`, optional `max_tokens`; emits `token` and final `llm_result`.
- STT: `transcript_hint`, optional `language`; emits `partial_transcript` and `transcript`.
- TTS: `text`, optional `sample_rate_hz`; emits bounded base64 PCM16 `audio_chunk`, `alignment`, and `tts_result`.
- Embedding: `texts`; emits normalized deterministic vectors.
- Vision: `frame_digest` and optional `labels`; emits deterministic detections derived from the digest.
- Lip-sync: `npc.mouth-residual/v1` metadata with opaque expiring frame/audio lease IDs, selected local encounter/track ID and track epoch, source frame sequence/capture QPC, QPC frequency, normalized face/landmark/mouth-mask bounds, tracking confidence, presentation deadline, and the matching cancellation generation. It emits deterministic `mouth_patch_proposal` metadata and a result. It never receives inline media, emits pixels, or edits images.

These results exist only for orchestration and UI simulation. They are not suitable for quality, latency, WER, FPS, or resource benchmarks.

## Generic current-frame lip-sync

`mouth-residual-v1.schema.json` is the normative JSON shape. This is a **live current-frame residual** contract, not a talking-head or static-avatar contract. A conforming worker may output only an alpha-bearing mouth residual for composition over the identified live source frame. It must not return a replacement frame, synthesized head, cached avatar frame, or any claim that static-avatar performance predicts moving-game performance. The untouched captured frame remains authoritative.

The required development JSON payload is:

```json
{
  "contract_version": "npc.mouth-residual/v1",
  "frame_lease_id": "frame-lease-000042",
  "audio_lease_id": "audio-lease-000017",
  "frame_lease_expires_qpc": 1300000,
  "audio_lease_expires_qpc": 1300000,
  "selected_encounter_id": "encounter-local-7",
  "selected_track_id": "track-local-2",
  "track_epoch": 3,
  "source_frame_sequence": 42,
  "source_capture_qpc": 1000000,
  "qpc_frequency_hz": 1000000,
  "face_region_normalized": { "x": 0.25, "y": 0.15, "width": 0.4, "height": 0.6 },
  "landmark_bounds_normalized": { "x": 0.29, "y": 0.21, "width": 0.32, "height": 0.48 },
  "mouth_mask_bounds_normalized": { "x": 0.38, "y": 0.55, "width": 0.14, "height": 0.1 },
  "tracking_confidence": 0.94,
  "presentation_deadline_qpc": 1200000,
  "cancellation_generation": 0
}
```

All QPC values come from the supervisor's single clock domain. The presentation deadline must be later than capture and no later than either lease expiry. Landmark bounds must be inside the tracked face; the mouth mask must be inside the landmark bounds; all three must be inside the normalized frame. `track_epoch` changes whenever the tracker loses/reacquires or rebinds an identity, even if its opaque track ID string is reused. `cancellation_generation` must equal the envelope generation.

Unknown fields are rejected. In particular, fields such as `image`, `image_b64`, `pixels`, `audio`, `audio_b64`, `frame_path`, `audio_path`, `url`, or a raw shared handle cannot enter the JSON control plane. Production transports can associate an opaque lease ID with a bounded shared-memory or D3D resource inside the already authenticated supervisor/media channel.

The deterministic proposal is deliberately non-presentable:

- `patch_lease_id` is `null`, `metadata_only` is true, and `image_modified` is false;
- `no_pixels_inline` is true;
- the exact source frame sequence, capture QPC, track ID/epoch, encounter, landmark/mask bounds, confidence, and cancellation generation are repeated;
- freshness names `valid_source_frame_sequence`, sets `discard_at_or_after_frame_sequence` to source + 1, permits zero source-frame advance, and permits at most one displayed frame;
- source-frame, track-epoch, deadline, lease, generation, landmark, mask, occlusion, and confidence failures all fail open to the untouched game frame;
- residual constraints set `full_frame_replacement`, `static_avatar_source`, and `base_frame_mutation` to false and require zero alpha outside the mouth mask.

A production worker may provide an opaque `patch_lease_id` only through a separately qualified contract implementation. That lease may contain only the declared mouth residual; a full-sized allocation does not grant permission to replace the frame. The compositor must reject a late, one-frame-stale, cancelled, mismatched, low-confidence, occluded, identity-switched, or out-of-bounds proposal and present the untouched current frame. Failure of the optional visual path never pauses conversation, audio, or subtitles.
