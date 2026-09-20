# Continuation audit — 20 September 2026

The September 17 implementation was reviewed rather than reverted wholesale. This note supersedes its completion claims where stated below. The product requirements and user preferences in `NEXT_AGENT_HANDOFF_2026-09-16.md` still apply.

## GitHub

After the user authorized GitHub CLI as AkshitIreddy, remote `main` was fast-forwarded to the already published `feat/2.0-overhaul` commit `4874ca9fc4d00949b7092b5a50831ff69d2683ef`. At the user's subsequent request, both branches received documentation commit `146ea50`: the old main README from `503ef3b` is restored byte-for-byte, and the overhaul guide is retained as `README-2.0.md`. Today's local repairs and the previously unpushed September 17 commits were not included in those updates. No release or tag was created. The README commit skips CI; no workflows were running when checked afterward.

## Kept

- The manual test-game attachment approach, including detailed connection errors. The implementation needed the fallback corrections below.
- The Groq catalog rotation, supported by the retained September 17 provider report. No new provider latency measurement was made in this audit.
- Correct official ONNX Runtime 1.22.1 binary pins and the installed review vision-pack resolver.
- YuNet preference for the practice game with fail-closed handling of corrupt installed packs. Retained detector evidence reports 42/42 accepted frames, versus 1/42 for mnv3. This measures detection, not successful lip animation or natural appearance.
- The credential-free Testing providers, with their evidence semantics corrected below. The Testing voice produces tones, not natural speech.

## Repaired

1. A closed review metadata file no longer blocks attachment through a valid production-namespace handoff. A dead primary target can retry that alternate only after native `invalid_window` or `process_mismatch` rejection. Corrupt primary metadata still fails closed. Cached target selection no longer vetoes a newly validated native identity and is not described as proof of liveness.
2. In the review visual coordinator, an inactive first sentence lease no longer prevents reaching a later active sentence. The new regression test failed before the change and passed after it. This corrects scheduling; it does not establish live rendering quality.
3. Visual diagnostics now distinguish Off, no returned receipt, inactive audio, missing target, capture authority, worker failures and broker stages. Diagnostic labels are bounded typed codes, not raw worker messages. A started event records that the visual loop was requested.
4. The practice game now respects the user's Lip-sync Off preference. The inherited debug-only override was removed. Enable lip-sync explicitly when reviewing mouth motion.
5. A Testing voice measured only in memory no longer claims speaker submission or drainage. Without an output lease it falls back to subtitle delivery. Only native broker/WASAPI submission receipts cross as endpoint audio evidence. Testing results are explicitly dummy/non-live. Broker-leased Testing requests still use the real endpoint path. Executed selected-route turns do not replay the old fixture's artificial stage delays, even when the providers are Testing dummies.

## Fresh verification

All checks were headless, without launching the review application, game, flashing native smoke suite or audio playback.

- Frontend: 182 tests passed (`E:\temp\InteractiveNPCs\frontend-audit-20260920.json`).
- Manual-attachment metadata/fallback: three focused tests passed.
- Visual runtime: 37 passed, two installed-model checks ignored; includes the sentence scheduling and typed failure-code regressions.
- Router disabled/missing-receipt distinction: one focused test passed.
- Router Testing timing: one focused test passed; no artificial demo delays for selected-route execution.
- Testing providers: two integration tests and two focused evidence unit tests passed.

## Evidence limits

The previous handoff's `live/loopback-turn3.wav` was absent when checked. It must not be cited as retained audible-output proof. The archived live captures still do not establish successful mouth articulation. Today's tests establish the repaired control flow and evidence semantics, not an end-to-end visible or audible result. No new GPU or provider benchmark was run.

The stable local delivery folder remains `E:\temp\InteractiveNPCs\review`. Its `REVIEW-MANIFEST.json` and the sibling `review-delivery.md` identify the actual built source, hashes and latest package checks. Do not infer package freshness from this source note alone.

## Mouth packs

See [the current alternatives assessment](../research/pack-optional-lipsync-2026-09-20.md). Manual packs can plausibly become optional through automatically collected/generated references or a direct frame-conditioned model. A closed-mouth first encounter still needs convincing unseen oral anatomy. The shared-interior approximation and NVIDIA route remain hypotheses to test, not implemented quality claims. Existing private character packs remain unqualified and disabled for ordinary targets.
