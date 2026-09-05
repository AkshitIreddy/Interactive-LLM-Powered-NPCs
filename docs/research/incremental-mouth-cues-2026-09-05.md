# Incremental mouth-cue experiment

## Decision

A bounded rolling Rhubarb window can publish immutable, sample-clock mouth cues long before a paced 9.38-second utterance has fully arrived. It is **not a good default replacement for the immediate audio-driven mouth path**. On this host, the first commit-stable cues appeared about 1.45 seconds after the first PCM sample, but a cue-only player needed about 2.75 seconds of initial buffering to avoid outrunning subsequent CLI passes. The incremental timeline also differed from a full-utterance Rhubarb pass on 12.40% to 14.52% of PCM frames.

The practical use is an optional look-ahead enhancement when TTS delivers audio substantially faster than playback. The product should keep its local audio-driven mouth response at audio start. If it later adopts this recognizer, it should accept Rhubarb cues only for future, unplayed sample intervals and retain the audio-driven state wherever the recognizer lacks enough lead. It must never rewrite a character interval that the player has already presented.

This is a standalone CPU experiment. It is not wired into runtime-host, the native mouth worker, the review application, or the test game.

## Prototype

[`scripts/benchmarks/benchmark-incremental-mouth-cues.py`](../../scripts/benchmarks/benchmark-incremental-mouth-cues.py) replays PCM arrival on a virtual wall clock and invokes the existing Rhubarb CLI on one window at a time. It uses:

- an initial 500 ms prefix;
- 250 ms snapshot opportunities, coalesced to the newest available snapshot while the preceding process is busy;
- a maximum 2,500 ms recognition window;
- a 250 ms uncommitted tail for right-context stability;
- the phonetic recognizer with one CPU thread; and
- integer PCM sample frames as the only publication clock.

Each result may append cues only from the ledger's prior commit cursor through the new watermark. The ledger clips the boundary cue at the watermark, validates contiguous coverage, and never mutates an existing interval. A generation mismatch or cancelled generation rejects the entire result. The subprocess wrapper kills and drains Rhubarb on cancellation or timeout.

The output report retains every invocation, its virtual start and finish time, exact recognized window, commit horizon, cues, input and executable hashes, and the standalone/replay classification. It refuses to replace an existing report unless `--overwrite` is explicit.

Example:

```powershell
python scripts/benchmarks/benchmark-incremental-mouth-cues.py `
  --audio 'E:\temp\InteractiveNPCs\action-demo-20260905\audio\elevenlabs-sarah-synthetic-lipsync.wav' `
  --rhubarb 'E:\temp\InteractiveNPCs\lipsync-quality-20260905\cues\runtime\Rhubarb-Lip-Sync-1.14.0-Windows\rhubarb.exe' `
  --out 'E:\temp\InteractiveNPCs\incremental-cue-probe-20260905\incremental-mouth-cues.json'
```

## Workload and identities

| Item | Measured input |
| --- | --- |
| Audio | ElevenLabs Sarah synthetic lip-sync WAV, PCM16 mono, 24 kHz |
| Audio frames / duration | 225,141 / 9.380875 s |
| Audio SHA-256 | `bbcbcfbade9e0dbacf4ccbfeddab3c659038fa397a40bc955b51133678d1667e` |
| Recognizer | Rhubarb Lip Sync 1.14.0, phonetic mode, one thread |
| `rhubarb.exe` SHA-256 | `9e289c6b5939ef6b306a61e8105ec721fd8d52b3ce950d08891ea3cc7df5718d` |
| CPU | 13th Gen Intel Core i9-13980HX, 24 cores / 32 logical processors |
| OS | Windows 11 Home Single Language, build 26200 |
| Python | 3.12.2 |
| Repository commit observed | `61ce86317a2835ae4a9ca4b4f044a7b578d78a33` with shared uncommitted work |

The benchmark used no GPU, provider API, audio device, GUI, or desktop capture. Process elapsed time is wall time; it is not isolated CPU time. Other work on the shared machine can influence which 250 ms snapshot is newest when a pass finishes.

## Measurements

Three sequential rolling-window repetitions used the same files and configuration. Values are measured from the virtual paced-arrival trace using each invocation's real process duration.

| Metric | Median | Observed range |
| --- | ---: | ---: |
| Full-utterance Rhubarb pass | 1,820.454 ms | 1,814.196–1,987.130 ms |
| First commit-stable cue from first PCM | 1,451.465 ms | 1,435.190–1,559.576 ms |
| Minimum cue-only playback delay without underrun | 2,750.614 ms | 2,746.799–3,021.223 ms |
| Final cue ready from first PCM | 11,495.945 ms | 11,117.965–11,550.459 ms |
| Sum of Rhubarb process time | 10,995.946 ms | 10,617.965–11,050.459 ms |
| Repeated-window processing RTF | 0.513975 | 0.511439–0.544511 |
| Recognition passes | 10 | 9–10 |
| Frames differing from the full pass | 12.7507% | 12.4029–14.5162% |
| Revisions to committed intervals | 0 | 0 |
| Final exact sample coverage | 225,141 / 225,141 | all runs |

“Commit-stable” means the prototype will not revise the published interval. It does not mean the partial classification matches later full-context recognition. The full pass is a consistency comparator, not phonetic or perceptual ground truth.

The 2.5-second windows themselves run faster than their audio duration, but repeated process startup consumes about 0.9–1.2 seconds per pass. The sum of process time is greater than the utterance duration because overlapping audio is intentionally reanalyzed. Only one Rhubarb process runs at a time.

The first 500 ms prefix produced enough recognition to commit the first 250 ms. That result became available around 1.45 seconds after the first sample, almost eight seconds before a paced source finished arriving. It was still too small a cue horizon to begin cue-only playback safely. The following passes determine the larger 2.75-second no-underrun delay.

### Growing-prefix comparison

One preliminary comparison used a 1,000 ms snapshot cadence. The rolling 2.5-second window needed 10 passes, 10,981.434 ms total process time, and a 3,319.090 ms safe playback delay; it differed from the full pass on 13.9513% of frames. Growing-prefix recognition needed 9 passes, 12,308.508 ms total process time, and a 4,642.248 ms safe delay; it differed on 10.8732% of frames.

Growing prefixes retain more context and were closer to the full pass in this one comparison, but their pass time grows with the utterance. The rolling window used less total recognition time and bounded each invocation's input. Denser, coalesced snapshot opportunities reduced rolling-window scheduling lag without launching concurrent recognizers.

Independent fixed chunks were not promoted to a prototype. They remove left and right context at every seam and still pay the same CLI startup floor. The rolling window already supplies overlap and an immutable watermark with less boundary risk.

## Integration consequence

Rhubarb 1.14 is a complete-file CLI. Prefix files make early results possible, but they do not turn it into a low-latency streaming recognizer. With real-time PCM arrival, recognition is causal: it cannot create future cues beyond audio it has received. Product integration therefore has two viable conditions:

1. TTS audio arrives several seconds ahead of playback, allowing the rolling recognizer to maintain its measured lead; or
2. the immediate local audio-driven mouth path remains authoritative until Rhubarb has cues for future, unplayed samples.

Adding a roughly 2.75-second mandatory playback buffer would spend too much of the user's 5–8 second input-to-character-response target. A persistent daemon might remove some CLI startup cost, but this experiment does not justify adding that lifecycle and security surface. Keep the prototype optional and standalone until a real provider PCM-arrival trace shows enough look-ahead on the target machine.

If integrated later, the runtime must native-stamp the generation, sample rate, first-sample playback epoch, and cancellation token. A result from an earlier generation must be dropped before cue publication. On cancellation, terminate the active recognizer, discard queued results, and leave the mouth in the existing safe audio-driven or neutral state.

## Verification

Focused tests are in [`scripts/benchmarks/test_benchmark_incremental_mouth_cues.py`](../../scripts/benchmarks/test_benchmark_incremental_mouth_cues.py). They cover immutable past intervals despite later disagreement, exact commit continuity, stale-generation rejection, cancelled-generation rejection, no-underrun scheduling math, exact disagreement accounting, and prompt termination of a pending child process.

```powershell
python -m unittest scripts/benchmarks/test_benchmark_incremental_mouth_cues.py -v
python -m py_compile scripts/benchmarks/benchmark-incremental-mouth-cues.py scripts/benchmarks/test_benchmark_incremental_mouth_cues.py
```

All six tests passed. The process-boundary cancellation test starts a sleeping child, cancels it, and verifies termination in under one second; it is not a mirrored descriptor test. `git diff --check` also passed for the three owned files.

Measured reports:

- `E:\temp\InteractiveNPCs\incremental-cue-probe-20260905\incremental-mouth-cues-v2.json`
- `E:\temp\InteractiveNPCs\incremental-cue-probe-20260905\incremental-mouth-cues-v3.json`
- `E:\temp\InteractiveNPCs\incremental-cue-probe-20260905\incremental-mouth-cues-v4.json`

The earlier growing-prefix comparison is retained at `E:\temp\InteractiveNPCs\incremental-cue-probe-20260905\incremental-mouth-cues-v1.json`.
