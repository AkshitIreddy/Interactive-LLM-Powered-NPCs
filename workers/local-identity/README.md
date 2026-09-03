# Local identity observation worker

This optional Windows worker is the isolated CPU inference boundary for known
character appearance observations. It can run YuNet detection and SFace
embedding extraction from a broker-owned, read-only BGRA shared-memory lease.
It does **not** decide who a character is.

`npc-identity-engine` owns sticky tracks, selected actor continuity, calibrated
multi-frame consensus, runner-up margins, ambiguity, explicit correction, and
offscreen behavior. The worker output is labelled
`untrusted_worker_observations`; native code must revalidate the exact capture
target, WGC frame sequence/QPC, device and geometry generations, content hash,
detector revision, preprocessing revision, and tensor model before constructing
`TrustedWgcIdentityFrameV1`.

## Safety and privacy boundary

- The hidden process authenticates a supervisor launch nonce and accepts one
  job at a time. `identity_worker_transport.rs` starts the fixed Python
  executable with `CREATE_NO_WINDOW`, null stderr, bounded piped stdio, and no
  inherited provider-key environment. It admits the PID only after assignment
  to the app's kill-on-close Job Object and an exact descriptor handshake.
- It opens only a bounded `Local\\npc.identity.<sha256>` mapping whose name is
  derived from the lease ID and nonce. The broker must make that mapping
  read-only to the worker and restrict it to the app user's SID. Pixels never enter the
  WebView, command line, logs, reports, or worker output.
- Network access is denied after startup. The worker never downloads a model.
- References must be explicitly `user_private` and local-only with consent, or
  `original_synthetic` with a recorded license. Every result is a versioned,
  normalized 128-dimensional f32 tensor; pickle and object deserialization are
  absent.
- No demographic, emotion, disability, or other protected-trait inference is
  implemented or allowed.
- Zero or multiple faces reject reference import. Low-confidence live faces
  remain observations; the Rust identity engine may retain the explicit actor,
  report ambiguity, or fall back to offscreen interaction.
- Cancellation advances an exact generation. If synchronous OpenCV inference
  misses the two-second barrier, the process marks itself faulted and exits so
  a supervisor can restart it safely. A native request timeout always replaces
  the child, re-authenticates it, replays only bounded cancellation generations,
  and restores an explicitly authorized load before accepting another frame.

## Native control contract

Frames are UTF-8 JSON preceded by an unsigned 32-bit big-endian byte length in
`1..=1,048,576`. Every request binds protocol version, worker instance, unique
monotonic sequence, exact cancellation generation, Unix-millisecond deadline,
operation, and payload. Every worker event must bind those same values and an
exact monotonic event index. Unknown fields, duplicate observation events,
oversize frames, stale generations, late results, and incomplete terminal
sequences fail closed.

Model load is never implicit. `activation_mode` is exactly
`private_evaluation` or `qualified_catalog`, and
`explicit_user_confirmation` is mandatory. Private evaluation rejects any
catalog-admission digest. Qualified catalog activation requires a lowercase
SHA-256 admission digest supplied by Model Manager. Both paths must match the
exact manifest and installed pack path; the current manifest remains blocked
pending signed current-device measurement, so no production catalog admission
exists yet.

Reference extraction is a separate additive native transport. It accepts no
path: the OS picker/control layer must decode and bound a selected regular
image, create a short-lived same-user read-only `Local` BGRA mapping, and pass
only that lease plus `QualifiedReferenceProvenanceV1`. The transport enforces
the mapping-name/hash/size binding and private-consent or original-license
rules, then strictly returns the existing `PortableReferenceImportV1`. The
control layer must release the mapping on every terminal path and immediately
import the result into the matching game-scoped gallery; image bytes and lease
capabilities are never persisted or returned to the WebView.

## Pack status

`packaging/model-packs/opencv-yunet-sface-private-evaluation.json` is the strict
v2 manifest. It pins the exact OpenCV Zoo commit, Git LFS SHA-256 values, byte
lengths, model notices, and runtime ABI. Nothing is bundled. YuNet has an exact
MIT directory license. The exact SFace model card says all files in its
directory are Apache-2.0, and a pinned OpenCV Zoo report says its model weights
were collected for any purpose including commercial use while identifying
SFace as an OpenCV Area Chair contribution. That report is itself a hashed pack
artifact. The model pack is therefore permissively licensed, but remains
explicit-download, unmeasured, and ineligible for the production signed catalog
until attestation, open-set calibration, and whole-loadout admission pass.
OpenCV and NumPy runtime wheels are separate components: their exact pins remain
in `runtime-requirements.windows-x86_64-cp312.v1.json`, and their extracted
third-party notices require separate runtime admission.

Deterministic tests use `DeterministicFixtureBackend`; constructing the normal
worker always selects the real OpenCV backend. Fixture tests do not download or
run a model and are not latency, accuracy, or game-compatibility evidence.

`qualification-plan.v1.json` freezes the future current-device benchmark:
at least 20 successful real samples each for cold load, reload, cancellation,
unload, and crash/restart; at least 100 advancing-frame samples for single- and
multi-face inference; process RAM/CPU and dedicated/shared GPU telemetry; game
frame-time impact; and a separate rights-cleared open-set quality corpus. The
plan is marked `prepared_not_executed`. `qualification.py` only validates raw
samples and builds an explicitly unsigned, non-admissible envelope candidate;
it cannot download/load a model, sign evidence, or mint Model Manager
admission.

Run the isolated contract suite:

```powershell
python -m unittest discover -s workers/local-identity/tests -p "test_*.py"
cargo test -p npc-identity-engine --test worker_fixture_contract
cargo test -p model-manager --test identity_private_pack
python workers/local-identity/qualification.py
```

Real-model qualification must wait for the API-first product path and the
repository GPU-lock protocol. CPU inference still counts as an AI-model run for
that coordination rule.
