# Local review v18 — 8 September 2026

The September 7 pause has ended. A fresh local application and test game are
built and verified. This is a private review checkpoint, not production or
general live-game lip-sync qualification. Nothing was pushed or released.

## Open and review

- Application: [interactive-npcs-control.exe](E:/temp/InteractiveNPCs/review-v18/interactive-npcs-control.exe)
- Setup and limitations: [REVIEW.md](E:/temp/InteractiveNPCs/review-v18/REVIEW.md)
- Four-character native comparison: [video](E:/temp/InteractiveNPCs/integration-20260907/native-review-final-v2-20260908/native-current-pixel-four-character-review.mp4)
- Actual test-game exported blinking/breathing frames with native mouth rendering: [video](E:/temp/InteractiveNPCs/test-game-blink-native-20260908/review/test-game-native-lipsync.mp4)

Start the control application, then use Guided setup → Meet Mara → Start &
connect test game. Mara's reviewed atlas is bundled. Canonical Misty and Claire
packs are staged disabled for optional import. Johnny is not offered as working
lip-sync: native landmark confidence fails and the output preserves the source.
No provider credentials were imported. Model metadata is staged, not an active
model installation. The private review catalog expires September 14 at 06:05:19 UTC;
refresh its signed admission metadata if reviewing after that time.

The videos use prerecorded speech, not a newly generated live conversation.
The four-character comparison is 10.4 seconds at 15 Hz. The test-game excerpt
is 1.4 seconds and includes a full game-owned blink, preserved outside the mouth.

## Frozen package and verification

Package directory: `E:\temp\InteractiveNPCs\review-v18`.
Built from clean branch `feat/2.0-overhaul`, source commit
`96855bb95e572ddf0f114663bb40ff6bd0ca1d27`. Later documentation commits do not
change this frozen package's source identity. Do not modify package contents.

Manifest SHA-256:
`d0e9b6d4799a6003eea4b211528822d74042f805d4c0dd803bb8b47eeff86cfa`.
The closed-world verifier passed for 769 files and six executables. Source
hygiene, current source/untracked secret scan, strict license/provenance,
GUI subsystem, test-game prerequisites, staged and final-directory checks passed.
Reachable-history secret scanning is not claimed by this manifest.

The final integration test used the delivered worker and test-game executables,
the exact reviewed Mara texture, native provider activation, and test-owned D3D
input. It passed; incorrect identity, changed texture, malformed wire, and stale
generation were rejected. Its isolated registry did not change product settings.

- [Packaged join receipt](E:/temp/InteractiveNPCs/schema4-native-join-proof-20260908-r2-packaged-v18/schema4-native-join-receipt.json)
- Receipt SHA-256: `6c81f70a6fdd7096315a5abe5e245cf84f1c54cc821fa5c1a6dca2dec0ea87bb`
- Worker SHA-256: `e9acf7cd62fd37933cf0d7888c3a2efe13345ebd086abef083e05d2df6c2f15e`
- Rust format and all-targets Clippy with warnings denied passed.
- Native source checkpoint passed all 12 CTest groups; native source was unchanged this continuation.

All verification was hidden/headless, with no desktop capture, physical audio
playback, visible test-game window, or control-app launch. Both helper agents
finished and reported no remaining owned worker/test-game processes.

## Visual and timing evidence

Final rerenders matched all 156 preceding frames exactly. Enlarged mouth boards
were inspected; 16 bilabial residuals retained contact and no pixels outside the
mouth ROI changed. Misty, Claire and Mara remain bounded private visual reviews;
natural quality and ordinary target enablement remain false.

| Case | Native residual frames | Worker + composition p95 |
| --- | --- | --- |
| Misty | 45/45 | 11.493 ms |
| Claire | 45/45 | 6.208 ms |
| Mara | 21/21 | 18.258 ms |
| Johnny | 0/45; exact-source bypass | Not qualified |
| Actual test-game blink export | 21/21 | 15.535 ms |

These timings exclude landmark inference, capture, presentation, game contention,
audio decoding and file writes. They are not conversational latency measurements.
The 6.15-second integration test duration is also not response latency.

Final visual receipt:
`E:\temp\InteractiveNPCs\integration-20260907\native-visual-review-20260908.json`
SHA-256 `7ccbdd5cb081328076ff00244cff39540fef03440385e7e2770a6b54a85644d8`.
The final video directory includes the exact assembler source snapshot and
verification receipt. Headless decoded audio matched the source at zero lag
(correlation 0.999809); no listening claim is made.

The additional actual test-game export uses 42 source frames beginning at 1.8s,
with native landmarks accepted on all 42 and 21 rendered frames. Its export,
landmarks, frame mapping, native report and review receipt are under
`E:\temp\InteractiveNPCs\test-game-blink-native-20260908`. This supplementary
evidence was created separately and is not inside the frozen package.

## Remaining acceptance work

Do not equate the tested components with an end-to-end live game session.
Production selected-character authority, fresh target-PID loadout admission,
commercial-game WGC capture, WASAPI playback envelope, overlay presentation,
contention and generic moving characters remain open. Johnny tracking remains
unqualified. Preserve the existing prohibition on visible/capture/audio testing;
do not quietly relax it to close these gates. No new hosted-provider latency
benchmark was run in this continuation.

For implementation detail, continue from
[native integration](docs/research/native-current-pixel-integration-2026-09-07.md).
The old [pause file](PAUSED_PROGRESS_2026-09-07.md) is historical context.
