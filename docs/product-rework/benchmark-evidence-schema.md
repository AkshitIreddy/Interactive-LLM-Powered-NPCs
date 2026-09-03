# Benchmark evidence schema v1

Status: implementation contract  
Date: 2026-08-30  
Machine schema: `benchmark-evidence-v1.schema.json`

## Purpose

This schema makes benchmark claims auditable without persisting provider
content or secrets. `scripts/benchmarks/benchmark_harness.py` converts strict
numeric JSONL observations into one aggregate JSON report. The report always
contains p50, p95, and p99 for capture, identity, LLM first-token and token
throughput/count, TTS first-audio and completion, audio submission, compositor
latency/FPS/stale drops, and end-to-end turn latency.

## Evidence classification

Classification has two axes and neither may be inferred from the other:

| Field | Values | Meaning |
| --- | --- | --- |
| `execution_mode` | `simulated`, `mocked`, `live` | What system path produced the observations |
| `measurement_kind` | `measured`, `planning_estimate` | Whether values were observed or are a forecast/budget |

Examples: `simulated + measured` is a real measurement of a deterministic
simulation, not a live product result. `live + planning_estimate` remains only a
plan. `live + measured` is reviewable evidence only when the harness itself ran
on Windows. The report records this as `classification.acceptance_eligible` and
also gives a plain reason. Eligibility never replaces scenario, artifact, or
acceptance-ledger review.

## Collection boundaries

- Use a monotonic clock at each component boundary; convert to milliseconds
  only when writing an observation.
- `tts.ttfa_ms` ends when decoded audio is ready for submission, not when the
  HTTP body starts or an encoded packet arrives.
- `audio.submission_ms` ends at the OS endpoint submission boundary. It does
  not claim physical audibility.
- `end_to_end.turn_ms` ends at first response-audio submission. Final TTS audio
  is separately captured by `tts.audio_completion_ms`.
- `compositor.stale_drops` is a counter per fixed measurement window; the same
  window definition must be used throughout a report.
- A report must use one execution mode and one measurement kind and must contain
  at least one observation for every required metric. Do not merge unlike
  machines, model/loadout revisions, capture profiles, or test scenarios into
  one input file.
- The product This-PC coordinator additionally requires
  `provider_observation_source=product_runtime_receipt`. Its runtime turn receipt
  binds the selected LLM provider/model/route revision and canonical egress,
  structured-response validation, selected TTS provider/model/stock voice/route
  revision and canonical egress, first decoded PCM and final decoded PCM
  timestamps, and a terminal cancellation probe. Historical live qualification
  files and deterministic fixtures are deliberately non-promotable.
- p50/p95/p99 use R-7 linear interpolation with zero-based position
  `(sample_count - 1) * q`. Values are rounded to six decimal places.

## Privacy and safety

The input row allowlist is deliberately narrower than the output schema. Rows
contain only schema version, bounded opaque sample/turn IDs, metric, numeric
value, unit, UTC observation time, execution mode, and measurement kind.
Unknown fields are rejected. Prompts, transcripts, generated text, audio,
credentials, headers, endpoints, file paths, usernames, hostnames, environment
variables, arbitrary tags, and provider response bodies do not belong in this
format.

The report stores only aggregate results, safe machine characteristics, input
byte/record counts, and the input SHA-256. It sets `path_recorded`,
`payloads_recorded`, `hostname_recorded`, and
`environment_variables_recorded` to false as explicit invariants.

## Bounds and failure semantics

The Windows wrapper and Python entry point accept bounded runs. Defaults are 60
seconds, 100,000 records, and 16 MiB input. Hard ceilings are 900 seconds,
1,000,000 records, and 256 MiB. Simulation iterations are also bounded. Parsing
checks elapsed monotonic time and file size before aggregation. Output is
written atomically only after all required metrics and classifications pass.

A bound violation, invalid JSON/UTF-8, non-finite or negative number, incorrect
unit, unknown field, mixed classification, missing metric, or invalid ID exits
nonzero. Errors identify the line or field class but never echo raw row values.
An incomplete file is not evidence.

The standalone JSONL harness remains useful for deterministic percentile
parity, but it is not the product receipt authority. Its `live + measured`
classification alone cannot promote a historical provider smoke into a This-PC
product result. Installed-app, selected-game-load, runtime receipt, broker audio,
and game-frame evidence must come from the native product coordinator.

## Versioning

Consumers must require the exact
`interactive-npcs-benchmark-report/v1` schema version. Additive metric or field
changes require a new schema version because v1 rejects unknown properties.
The JSON Schema validates structure; the harness additionally enforces unique
required metric coverage, metric-to-unit correspondence, uniform evidence
labels, classification eligibility, finite numbers, and execution bounds.
