# Native current-pixel integration

**Paused by the owner on 7 September.** Resume from
[PAUSED_PROGRESS_2026-09-07.md](../../PAUSED_PROGRESS_2026-09-07.md).
The final native build passed all 12 test groups. The replay results below
predate that final build and may predate the reversed-corner correction;
rerender before treating them as proof of the final binary. The final reviewed
Mara Rust/native join passed separately at `r7-reviewed-mara`. The v18 package
has not been built.

The owner accepted the visual improvement in the three-character v10 video
and requested integration on 7 September. That acceptance applies to the
offline comparison. The native route must be checked separately against its
own landmark provider, sample clock, and presentation contracts.

## Implemented boundary

Atlas schema 4 uses `normalized-oral-strip-v1`. It carries eroded oral pixels,
one exposure-context value per state, and an explicit source-edge refinement
policy. The native renderer inverse-maps ordered lip strips from the current
source frame. It admits reference pixels only inside the eroded aperture and
uses current lip surfaces for bilabial contact. Entirely transparent textures
select the source-only mode with an opening limited by visible source coverage.

Schemas 1–3 keep their existing meanings and byte layouts. Schema 4 adds a
little-endian f64 reference context and a 0/1 geometry-policy byte after each
state's pose and before its pixel count. All states must agree on the geometry
policy. The Rust importer/encoder and native decoder validate the new fields.
The character and cancellation-generation bindings remain authoritative.

The worker selects this renderer only for a matching schema-4 atlas. It uses
a continuous sample-clock trajectory for coefficients, preserves exact
bilabial contact, and resets motion on cancellation, actor/track changes, and
segment changes. It does not blend previous rendered RGB into the current
frame. The existing broker supplies a currently available timed cue snapshot;
no complete future utterance is required. The trajectory's optional future
metadata path is bounded to 120 ms, with 50 ms anticipation by default. This
does not add an audio buffering delay or promise provider lookahead.

For schema 4, tracking follows the newest face translation, scale and roll.
The adapter compares mouth placement relative to the independently tracked
face, retaining its existing confidence and mouth-only displacement limits.
A 25 ms filter damps only mouth-local shape, with correction bounded to 1.5%
of the current mouth width. It does not delay the actor's head position.
Legacy atlas routes retain their previous tracking behavior.

## Native tracking evidence

The first complete full-frame YuNet640 + OpenSeeFace LM1 diagnostic pass used
the same three 90-frame source sequences as the accepted comparison, with
the actual native provider and unchanged confidence gates. The v2 dumps in
`E:\temp\InteractiveNPCs\integration-20260907` contain all 66 measured points;
the replay converter refuses older mouth-only dumps rather than inventing the
remaining landmarks.

| Character | Native packets | Adapter accepted | Mean oral-point confidence |
| --- | ---: | ---: | ---: |
| Misty | 90/90 | 85/90 | 0.875 |
| Claire | 90/90 | 90/90 | 0.851 |
| Johnny | 90/90 | 0/90 | 0.688 |

Johnny's result is a native landmark qualification failure, not successful
lip-sync coverage. The existing 0.82 whole-face confidence floor rejected his
packets. Lowering that gate would not establish correct geometry. His model
alternatives were compared without changing that floor. Exact-input replay
matched native LM1 confidence within 1.37e-7. LM3 accepted 24/90 Johnny frames;
LM4 accepted 31/90. LM4 with contrast normalization and a tighter crop accepted
75/90, while retaining 90/90 for Misty and Claire. This is an experimental
candidate, not a qualified replacement provider. Model inference alone was
about 24–25 ms p95; full-frame contrast preprocessing adds 16–18 ms p95.
Enlarged candidate boards still show contour deformation and high-confidence
false mouth geometry, so higher acceptance alone does not qualify this route.
See [the provider diagnosis](native-landmark-provider-diagnosis-2026-09-07.md)
for exact-input parity, model comparisons and the separately timed overheads.

The retained native landmark board also shows that Misty's standing-view raw
inner contour can include closed vermilion. The source-edge regression must
therefore remain part of native visual verification, not just the Python
prototype's tests.

## Current native render results

The final replay uses source indices 0, 2, 4 and so on at the native 15 Hz
admission cadence. It runs exact full-66 native packets through the current-pose
adapter, schema-4 worker, sample-clock coefficients and CPU composition.

| Character | Residual outputs | Changed outputs | Source-exact bypasses | Worker + composition p95 |
| --- | ---: | ---: | ---: | ---: |
| Misty | 45/45 | 39 | 0 | 10.303 ms |
| Claire | 45/45 | 39 | 0 | 6.024 ms |
| Johnny | 0/45 | 0 | 45 | Not applicable |
| Mara test fixture | 21/21 | 20 | 0 | 16.369 ms |

All 16 rendered bilabial samples satisfy exact contact. No output changes
pixels outside its residual support. Transparent silence is counted as a
source-exact residual, not as visible lip movement. These are short CPU replay
measurements, excluding landmark inference, capture, audio decode, presentation,
game load and file encoding. They are not conversational latency figures.

The 11 native test groups pass, including reversed OSF contour ordering,
source-edge false-cavity repair, local-shape filtering, whole-face movement,
mouth-only jump rejection, schema-4 protocol validation and cancellation.
The Rust parser's 30 focused tests and the opt-in real-process join also pass.
The latter installs schema-4 bytes in a hidden native worker, rejects malformed
and stale data, runs the owned D3D service smoke, and self-tests the hidden game.
Its latest receipt is
`E:\temp\InteractiveNPCs\schema4-native-join-proof-20260907-r5\schema4-native-join-receipt.json`
(SHA-256 `428a02dcf6a0814e6fc5e7ae15171152d258ee0d88c1210ed7d5369e282c5dfa`).

## Evidence boundaries

The native signal adapter has a hard 15 Hz admission cap. A replay must state
its operational cadence and cannot present alternating source/modified 30 Hz
frames as smooth lip-sync. Current-frame tracking, model inference time,
rendering time, and file encoding are separate measurements.

The source-frame appearance guard remains diagnostic-only. Neither native
confidence nor manually excluded smoke frames establish a semantic occlusion
detector. Its small corpus detects the manually identified Johnny smoke span,
but also suppresses six clean latch/recovery frames and needs six warmup observations.
That evidence is insufficient for a default production visibility gate.
Private pack review does not automatically activate an ordinary game session.

The local test game uses fictional Mara Venn in Eclipse Harbor. Its assets and
semantic identity are distinct from the Cyberpunk characters. New test-game
mouth data must be prepared for Mara instead of renaming another character's
pack. No package, setup activation receipt, or headless component test grants
commercial-game actor-lock, capture, audio-device, or overlay authority.
