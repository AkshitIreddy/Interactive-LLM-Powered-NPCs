# Known-character identity worker qualification

Status: implementation architecture and private-evaluation candidate; not a
production accuracy, latency, licensing, or arbitrary-game support claim

Evidence refreshed: 2026-08-30 UTC

Target: a local Windows companion beside a game, with API-first speech and
dialogue, a 12 GB gaming GPU reserved for the game, and conservative explicit
character selection as the fallback

## Decision

The selected experimental stack is OpenCV Zoo YuNet 2026may for face detection
plus SFace 2021dec for a 128-dimensional appearance embedding, using OpenCV
5.0.0 CPU DNN only. It is a good engineering fit because the detector is tiny,
both models share the official OpenCV `FaceDetectorYN` / `FaceRecognizerSF`
alignment path, inference consumes no dedicated model VRAM, and the stack can
run in a queue-depth-one hidden process without competing with a game for the
GPU.

It is now a permissively licensed model-pack candidate, not a production-
qualified pack. YuNet's exact directory has an MIT license. SFace's exact model
card says all files in its directory are Apache-2.0. A pinned OpenCV Zoo report
goes further: it says the Zoo model weights were collected for use for any
purpose including commercial use and identifies SFace as an OpenCV Area Chair
contribution. The exact report is retained as a hashed notice artifact beside
the model card and licenses. This is sufficient model-file provenance for the
MIT/Apache pack facts; the SFace paper's named research datasets remain relevant
to quality, bias, and fitness evaluation, but do not silently replace OpenCV's
express license for its contributed weight. The application still does not
bundle or silently download either weight.

Production activation remains blocked for a different reason: no real self-
test, signed current-device resource envelope, rights-cleared open-set game
calibration, whole-loadout admission, or separately admitted OpenCV/NumPy
runtime exists yet. Those unknowns still fail closed.

The product decision remains explicit-first:

- the worker emits detections and versioned embeddings, never a character ID;
- the native broker revalidates exact target and WGC frame evidence;
- the Rust identity engine owns sticky actor tracks, open-set rejection,
  calibrated multi-frame consensus, runner-up margin, hysteresis, explicit
  correction, and offscreen continuity;
- ambiguity retains the current explicit selection or asks the player;
- a background actor gets a deterministic encounter ID and game-authored
  archetype/voice, never a guessed protected trait;
- if the pack is absent, unmeasured, ambiguous, stale, unsafe, or unavailable,
  typed/PTT interaction, audio, subtitles, and manual character selection
  continue.

## Immutable candidate record

OpenCV Zoo source commit:
`47534e27c9851bb1128ccc0102f1145e27f23f98`

| Artifact | Bytes | SHA-256 | Current permission decision |
| --- | ---: | --- | --- |
| `face_detection_yunet_2026may.onnx` | 229,738 | `ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0` | Exact directory MIT; explicit upstream download candidate |
| YuNet `LICENSE` | 1,085 | `c83b8120c50ccbd4c4f96edf53141bdd566ebb8f8e9227e415326aa1b1aba958` | Must remain beside model |
| `face_recognition_sface_2021dec.onnx` | 38,696,353 | `0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79` | Exact directory Apache-2.0; pinned Zoo report permits any purpose including commercial use |
| SFace `LICENSE` | 11,358 | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` | Exact directory Apache-2.0 claim retained |
| SFace model card | 2,267 | `e090e126f3fb5c0b0b2d84ee644b17e42a70b2e45a90616814970b811e91d063` | Retained; says every directory file is Apache-2.0 |
| OpenCV Zoo model-use report | 4,649 | `bd1ccd892cfd306829fe219de9950b3d97847dbb7d5cfdf3453361701c089ec3` | Retained; says Zoo weights may be used for any purpose including commercial use and names SFace's contribution source |

The exact pack artifact payload is 38,945,450 bytes, including the model-use
report. The strict v2 pack manifest at
`packaging/model-packs/opencv-yunet-sface-private-evaluation.json` has file
SHA-256
`a4af4874af77c4dc517fe41990371e96e68a4469ee6817102876b7b074302e67`.
The Windows CPython 3.12 runtime lock separately records
`opencv-python-headless==5.0.0.93` and `numpy==2.5.2` by exact wheel URL, byte
length, and SHA-256. The OpenCV wheel carries a large third-party notice file;
that full extracted inventory and its license obligations are a packaging gate,
not something a top-level Apache label replaces.

The chosen 2026may YuNet graph is the official dynamic-height/width export for
OpenCV 5.x. The older 2023mar graph remains useful for OpenCV 4.x but is not the
reviewed ABI here. SFace preprocessing is frozen as the official five-landmark
`FaceRecognizerSF.alignCrop` result, BGR 112x112 model input, `feature`, finite
f32 canonicalization, and L2 normalization. Changing detector, landmark order,
alignment, channel order, crop geometry, normalization, OpenCV revision, or
model bytes creates a different embedding space and requires a new gallery.

## Runtime and authority design

1. Native target selection owns PID, HWND, executable basename, capture session,
   anti-cheat/protected-online policy, and overlay exclusion.
2. Windows Graphics Capture provides an advancing Direct3D frame with QPC
   timestamp. Device recreation and resize advance explicit generations; old
   work cannot cross either boundary.
3. The broker copies only the current bounded BGRA frame into a read-only named
   shared-memory lease. The lease has a random nonce, exact byte length, stride,
   content SHA-256, expiry, target and generation binding. Pixel bytes never
   enter the WebView, logs, reports, command line, or worker output.
4. A hidden, authenticated worker opens the mapping, re-hashes its exact bytes,
   runs YuNet, aligns each accepted face, and extracts SFace. Network access is
   denied and the worker cannot download, search arbitrary directories, or
   deserialize objects.
5. The worker returns `untrusted_worker_observations`: exact frame binding,
   detector-local IDs, finite boxes/confidences, and
   `NormalizedEmbeddingV1`-shaped values. Its output never says `trusted`, never
   supplies a character ID, and never echoes a mapping name or nonce.
6. Native code revalidates the target, frame/QPC, device and geometry
   generations, content digest, detector ID/revision, preprocessing, model
   revision, tensor dimension, normalization, safety flags, deadline, and
   cancellation generation. Only then may it construct
   `TrustedWgcIdentityFrameV1`.
7. `ActorIdentityEngineV1` associates observations into sticky tracks. Motion,
   intersection-over-union, appearance similarity, selected-actor bias, and
   bounded missed-frame windows reduce switches. Track epoch invalidates work
   across loss/reacquisition boundaries.
8. Recognition is open-set. A known subject is not admitted until several
   frames agree above a game/calibration-fixture threshold and top-1 clears the
   runner-up margin. Hysteresis can retain a prior lock but cannot silently
   replace an explicit assignment.
9. Ambiguous/no-match observations preserve explicit selection. Manual
   correction is authoritative until cleared or track retirement. A selected
   track may remain offscreen for a longer bounded interval, retaining its last
   explicitly confirmed subject without pretending it is visible.
10. Queue depth is one. A cancellation generation suppresses all late results.
    OpenCV DNN is synchronous, so a missed two-second barrier faults and exits
    the worker; the supervisor restarts a fresh process rather than accepting a
    result from a stale generation.

## Reference import and storage

Reference import supports only two closed provenance classes:

- `user_private`: explicit consent, a local owner ID, local-only storage, exact
  source-content SHA-256, and no provider egress;
- `original_synthetic`: local-only storage and a non-empty license record, with
  no private-user ownership claim.

The worker rejects zero or multiple detected faces instead of selecting the
highest score. A valid single face becomes a 128-value finite L2-normalized
tensor encoded as raw little-endian f32 bytes. `PortableReferenceImportV1`
binds tensor SHA-256, model/revision, detector/revision, preprocessing, crop,
source digest, game profile, subject, reference ID, consent/license, and import
time. JSON arrays are the portable transport. Pickle, Python object graphs,
arbitrary class names, executable model payloads, web-scraped references, and
unconsented reference folders are structurally absent.

Reference images and embeddings are sensitive local data. Removal must delete
the source copy if the product imported one, the tensor, gallery indexes, and
derived caches for that reference ID. Diagnostics may report counts, revisions,
and result states but not pixels, names, embeddings, tensor hashes that could
act as cross-report correlators, or filesystem paths.

## Calibration, tracking, and failure policy

OpenCV's tutorial thresholds are examples for its named benchmarks, not product
calibration. Face embeddings are not calibrated probabilities. Per-game and
per-render-style calibration must use an original/licensed fixture with known
matches, near-lookalike impostors, unknown actors, UI/HUD false positives,
occlusion, pose, lighting, camera motion, scene cuts, and small faces. The
signed result must bind all artifact/preprocessing revisions and report at
least false match, false non-match, false identification, ambiguity, track
switch, fragmentation, and offscreen-retention errors at the selected operating
point.

Required admission rules:

- threshold and runner-up margin are both required;
- minimum consensus must cover multiple distinct advancing frames;
- repeated inference over one frozen frame never counts as consensus;
- detector confidence is only observation quality, never identity confidence;
- an embedding from a different model or preprocessing space is incompatible;
- a scene/device/geometry/track epoch change clears temporal evidence;
- a single face appearing in the same screen location after a cut is not the
  previous actor by default;
- low-resolution, extreme-pose, occluded, duplicated, or near-tied candidates
  become ambiguous/no-match;
- explicit selection is the safe fallback, including when the actor is
  offscreen;
- calibration must be repeated for materially different game art styles and
  the UI must label uncalibrated profiles unavailable, not guess a threshold.

The multi-stage design follows the tracking literature's convergence: motion
alone is fast but fragile; appearance reduces identity switches; low-confidence
detections can help continuity but must not become identity evidence; camera
motion and occlusion need explicit treatment; and detection, localization and
association must be measured separately. This product deliberately uses the
smaller existing Rust tracker instead of importing a second neural re-ID stack,
because SFace already supplies bounded appearance and the safety-critical
requirement is conservative rejection rather than maximum MOT leaderboard
recall.

## Current evidence and explicit gaps

Implemented and currently proven without model execution:

- immutable manifest parsing rejects mutable URLs, hash drift, broadened SFace
  permission claims, traversal, unknown fields, unsigned activation, absent
  measurement gates, and any byte drift from the reviewed strict v2 document;
- authenticated handshake, one-job scheduler, exact cancellation generation,
  shutdown, and fresh-process restart;
- WGC target/frame/device/geometry/QPC/content binding and unsafe-capture
  rejection;
- private/original reference provenance and portable f32le tensors;
- exact cross-language worker JSON to Rust observation deserialization;
- sticky two-face tracking across detector-order reversal;
- calibrated ambiguity, authoritative manual correction, and offscreen
  continuity;
- native rejection of detector revision and source-frame digest drift;
- no character/protected-trait output and no shared-memory secret echo.

Not yet proven and therefore not claimed:

- downloaded-model hash verification against the actual downloaded bytes on
  this PC;
- OpenCV 5.0.0.93 Windows load or inference;
- real reference import quality;
- this-PC p50/p95/p99 load, detection, alignment, embedding, cancellation,
  CPU/RAM/VRAM, or game frame impact;
- calibrated error rates across any real or synthetic game corpus;
- WGC broker named-mapping creation, ACL, expiry, nonce, and native revalidation
  wiring in the packaged app;
- UI import/inspect/delete/manual-correction controls;
- signed catalog admission, transitive runtime notices, installer staging, or
  clean Windows install;
- separate OpenCV/NumPy runtime distribution admission and extracted notices.

The current-device harness is prepared but not executed. Its immutable plan
requires at least 20 successful real samples each for cold load, warm reload,
generation cancellation, unload, and crash/restart, plus at least 100 samples
each for advancing-frame single- and multi-face inference. Raw evidence must
retain process RAM/working set/CPU, dedicated and shared GPU memory, copy/hash/
detection/alignment timing, game frame time, failures, and outliers. A separate
rights-cleared open-set corpus measures false accepts/rejects, hard negatives,
unknowns, ambiguity, association, manual correction, and offscreen continuity.
The aggregator can create only an unsigned non-admissible envelope candidate;
trusted signing, self-test attestation, and whole-loadout admission remain
external mandatory gates.

No model was downloaded or executed while this document and worker were built.
Even though the selected backend is CPU-only, the repository treats real AI
model inference as GPU-lock-coordinated work and defers it until the API-first
end-to-end path is complete.

## Qualification matrix before any promotion

- Windows 10 and 11 clean install, hidden process, no developer Python/Git/CMake,
  offline self-test, repair, rollback, uninstall, crash loop, and restart.
- Exact artifact, runtime wheel, extracted-file, SBOM, notice, signature,
  catalog, and installed-tree hashes.
- At least 30 sequences per major visual stratum: realistic, stylized, anime,
  low-poly, non-human, masks/helmets, facial hair, 20-300 px heads, extreme
  pose, lighting changes, motion blur, cutscenes, HUDs, dialogue portraits,
  scene cuts, crowds, near-lookalikes, duplicated NPC templates, occlusion, and
  actor exit/re-entry.
- Known, known-unknown, and never-seen actors; original/private references only;
  multiple references per known subject; references that differ in pose,
  costume, age of capture, crop, and lighting without demographic labeling.
- False-match/false-non-match/open-set identification curves, top-1/top-2
  margin distributions, reliability/calibration diagrams, ambiguity and manual
  correction rates, track switches, fragmentation, HOTA components, and
  offscreen errors.
- Stale, replayed, reordered, duplicated, wrong-target, wrong-HWND,
  wrong-executable, wrong-frame, wrong-QPC, wrong-device, wrong-geometry,
  wrong-detector, wrong-preprocessing, wrong-model, corrupt tensor, cancellation,
  deadline, mapping ACL, mapping expiry, resize, device loss, and worker crash.
- CPU threads 1/2/4 beside the synthetic game and representative real games;
  load and per-frame p50/p95/p99/max, private working set, CPU time, zero
  dedicated model VRAM, capture/broker overhead, game average/1%-low/p95/p99
  frame-time impact, and foreground speech-priority preemption.
- Human review of every false accept and identity switch. A false accept is
  more serious than an extra ambiguity prompt for this companion use case.

## Primary, official, and paper evidence ledger

The following 36 sources were assessed. Paper/vendor performance selects what
to test; it is not imported as this-PC evidence. Mutable landing pages are
recorded for background only when an immutable commit/artifact record is also
present.

1. [Pinned OpenCV Zoo commit](https://github.com/opencv/opencv_zoo/tree/47534e27c9851bb1128ccc0102f1145e27f23f98) — immutable source tree used by every model and notice URL.
2. [YuNet model card at the pinned commit](https://github.com/opencv/opencv_zoo/blob/47534e27c9851bb1128ccc0102f1145e27f23f98/models/face_detection_yunet/README.md) — dynamic 2026may model, WIDER results, input behavior, and directory license statement.
3. [YuNet MIT license at the pinned commit](https://github.com/opencv/opencv_zoo/blob/47534e27c9851bb1128ccc0102f1145e27f23f98/models/face_detection_yunet/LICENSE) — exact notice admitted with the detector.
4. [SFace model card at the pinned commit](https://github.com/opencv/opencv_zoo/blob/47534e27c9851bb1128ccc0102f1145e27f23f98/models/face_recognition_sface/README.md) — MobileFaceNet/SFace description, five-landmark alignment, published metrics, and directory Apache statement.
5. [SFace Apache-2.0 license at the pinned commit](https://github.com/opencv/opencv_zoo/blob/47534e27c9851bb1128ccc0102f1145e27f23f98/models/face_recognition_sface/LICENSE) — exact notice admitted with the weight.
6. [OpenCV 5 DNN face tutorial](https://docs.opencv.org/5.0/tutorials/dnn/dnn_face/dnn_face.html) — official detector/recognizer creation, alignment, feature, cosine/L2 examples, and benchmark-specific thresholds.
7. [OpenCV 5 face DNN API](https://docs.opencv.org/5.0/main_modules/objdetect_dnn_face.html) — exact `FaceDetectorYN` and `FaceRecognizerSF` method contracts.
8. [OpenCV 5.0.0 source release](https://github.com/opencv/opencv/releases/tag/5.0.0) — runtime family and source release identity.
9. [OpenCV source license](https://github.com/opencv/opencv/blob/5.0.0/LICENSE) — Apache-2.0 terms for the native library itself.
10. [opencv-python-headless 5.0.0.93](https://pypi.org/project/opencv-python-headless/5.0.0.93/) — official wheel version, Windows/Python compatibility, and published file metadata.
11. [opencv-python licensing guidance](https://github.com/opencv/opencv-python/blob/4.x/README.md#licensing) — wrapper, OpenCV, FFmpeg, and other bundled components have separate license layers.
12. [opencv-python third-party notices](https://github.com/opencv/opencv-python/blob/4.x/LICENSE-3RD-PARTY.txt) — the extracted wheel notice inventory that must be retained/reconciled.
13. [NumPy v2.5.2 license](https://github.com/numpy/numpy/blob/v2.5.2/LICENSE.txt) — BSD-3-Clause runtime dependency terms.
14. [YuNet paper](https://doi.org/10.1007/s11633-023-1423-y) — lightweight anchor-free detector design and reported 320x320 CPU measurement assumptions.
15. [SFace paper](https://doi.org/10.1109/TIP.2020.3048632) — hypersphere loss, noisy-data motivation, benchmark protocol, and named training datasets; it does not by itself map the exact Zoo ONNX file to a reviewed dataset license.
16. [WIDER FACE paper](https://openaccess.thecvf.com/content_cvpr_2016/papers/Yang_WIDER_FACE_A_CVPR_2016_paper.pdf) — scale, pose, and occlusion strata needed for detector qualification.
17. [LFW updated protocol](https://web.cs.umass.edu/publication/docs/2014/UM-CS-2014-003.pdf) — verification reporting constraints and why one benchmark accuracy is not an open-set product threshold.
18. [NIST IJB-C protocol summary](https://www.nist.gov/document/readmepdf-1) — mixed-media verification, open-set search, covariates, detection, clustering, and video tasks.
19. [NIST Face Challenges](https://www.nist.gov/programs-projects/face-challenges) — independent challenge scope including full-motion video.
20. [FaceNet paper](https://openaccess.thecvf.com/content_cvpr_2015/html/Schroff_FaceNet_A_Unified_2015_CVPR_paper.html) — embeddings as a versioned similarity space and the role of alignment; no weight from this paper is bundled.
21. [ArcFace paper](https://arxiv.org/abs/1801.07698) — angular-margin embedding motivation and label-noise caveats; not selected because available pretrained-pack rights are weaker for this product.
22. [Toward Open-Set Face Recognition](https://arxiv.org/abs/1705.01567) — a thresholded verification score is not sufficient evidence for open-set identification.
23. [Open-set face challenge paper](https://arxiv.org/abs/1708.02337) — unknown identities and false detector accepts make open-set identification harder than closed-set verification.
24. [Calibration of modern neural networks](https://proceedings.mlr.press/v70/guo17a.html) — raw model confidence is not automatically calibrated; calibration must be measured on a representative held-out set.
25. [NIST FRTE 1:1](https://pages.nist.gov/frvt/html/frvt11.html) — false-match/false-non-match operating thresholds, image-quality sensitivity, and current evaluation framing.
26. [NISTIR 8280 demographic effects](https://nvlpubs.nist.gov/nistpubs/ir/2019/nist.ir.8280.pdf) — fixed thresholds and image quality can produce differential errors; the product avoids protected-trait inference and requires conservative failure analysis.
27. [SORT paper](https://arxiv.org/abs/1602.00763) — simple online motion association is fast, but detector quality dominates tracking behavior.
28. [Deep SORT paper](https://arxiv.org/abs/1703.07402) — adding appearance reduces identity switches and improves continuity through occlusion.
29. [ByteTrack paper](https://arxiv.org/abs/2110.06864) — low-score detections can preserve tracks, but this product does not treat them as identity evidence.
30. [OC-SORT paper](https://arxiv.org/abs/2203.14360) — observation-centric correction reduces accumulated motion error through occlusion/nonlinear motion.
31. [BoT-SORT paper](https://arxiv.org/abs/2206.14651) — motion, appearance, and camera-motion compensation are complementary association signals.
32. [HOTA paper](https://arxiv.org/abs/2009.07736) — detection, localization, and association must be evaluated separately as well as together.
33. [Windows Graphics Capture guidance](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture) — frame pools, current frames, QPC `SystemRelativeTime`, resize/device recreation, and background processing.
34. [Windows named shared memory](https://learn.microsoft.com/en-us/windows/win32/memory/creating-named-shared-memory) — file mappings, `Local`/`Global` namespaces, view lifetime, and cross-process access; product ACL/nonce hardening goes beyond the sample.
35. [Windows process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags) — `CREATE_NO_WINDOW` for a console worker launched without a user-visible console.
36. [Pinned OpenCV Zoo model-use report](https://github.com/opencv/opencv_zoo/blob/47534e27c9851bb1128ccc0102f1145e27f23f98/reports/2023-4.9.0/opencv_zoo_report-en-2023-4.9.0.md) — official provenance statement that the Zoo model weights were collected for use for any purpose including commercial use; identifies SFace as an OpenCV Area Chair contribution.
