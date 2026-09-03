# Optional local Qwen3 dialogue worker

This directory contains the real, isolated production adapter for the optional
Qwen3 4B local-LLM path. It does **not** bundle a model, silently download one,
or make local inference the default. Hosted APIs remain the default product
route. A user must explicitly select and install this pack, and activation still
requires the product's signed catalog, attested self-test, and whole-loadout
resource admission.

## Frozen stack

| Unit | Immutable identity | Download bytes | SHA-256 |
|---|---|---:|---|
| Qwen3-4B-Instruct-2507 Q4_K_M GGUF | `unsloth/Qwen3-4B-Instruct-2507-GGUF@a06e946bb6b655725eafa393f4a9745d460374c9` | 2,497,281,120 | `3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597` |
| Qwen Apache-2.0 license | `Qwen/Qwen3-4B-Instruct-2507@cdbee75f17c01a7cc42f958dc650907174af0554` | 11,343 | `832dd9e00a68dd83b3c3fb9f5588dad7dcf337a0db50f7d9483f310cd292e92e` |
| GGUF conversion provenance | `unsloth/...@a06e946bb6b655725eafa393f4a9745d460374c9` | 11,207 | `c18c2d01645e709fcc0bfd593291d3dc43e51873f5af170a082c36ccd95988c3` |
| llama.cpp Vulkan runtime | `b10689@57291f2644af8c9df0dd8d44395881c5bdcf0ecd` | 34,905,903 | `3da600ff52a746d82e32a2ba3f0382e3bf782bd3a8fece661b646e1dee7ac1c6` |
| llama.cpp CPU runtime | same source revision | 18,134,786 | `be2092f2913cd90428b0f1a57b186ab66afe29bc698ea6fbe80f73ffdebd4bdc` |

The Vulkan qualification download is exactly **2,532,209,573 bytes**. The CPU
alternative is exactly **2,515,438,456 bytes**. The model pack's installed
payload is 2,497,303,670 bytes and its conservative atomic staging peak is
4,994,607,340 bytes. The qualified Vulkan archive expands to exactly 52 regular
files / 101,402,397 bytes. The CPU archive remains uninspected and therefore has
no invented expanded-size claim.

The model pack is Apache-2.0. The separately installed llama.cpp runtime is MIT.
Keeping those trust units separate prevents the model manifest from claiming an
inaccurate composite license and matches the existing model-manager candidate.

## No-download inspection and tests

From the repository root:

```powershell
python workers/local-llm/qualify.py `
  --install-root "$env:LOCALAPPDATA\InteractiveNPCs\ModelPacks" `
  --runtime-variant windows-x64-vulkan plan

$env:PYTHONPATH = "workers/local-llm"
python -m unittest discover -s workers/local-llm/tests -v
```

`plan` does not instantiate a network fetcher, create an installation, or touch
the GPU. Tests use only in-memory byte fixtures and tiny temporary ZIP files.

## Explicit local-review install

The following is a local-review transport exercise, not production activation:

```powershell
python workers/local-llm/qualify.py `
  --install-root "$env:LOCALAPPDATA\InteractiveNPCs\ModelPacks" `
  --runtime-variant windows-x64-vulkan `
  install --i-understand-downloads

python workers/local-llm/qualify.py `
  --install-root "$env:LOCALAPPDATA\InteractiveNPCs\ModelPacks" `
  --runtime-variant windows-x64-vulkan verify
```

Downloads use credential-free HTTPS, a redirect host allowlist, immutable source
revisions, exact content lengths, streaming SHA-256, private staging files,
same-volume atomic rename, and fail-closed cleanup. ZIP extraction rejects
traversal, links, reparse-like paths, reserved Windows names, case-fold
collisions, encryption, excessive members, and expansion bombs. Repair stages
and verifies a complete replacement before switching; removal first requires an
unreferenced result and renames into a protected trash area.

## Supervised runtime

The worker implements Worker Control v1 over `u32be length || UTF-8 JSON`:

- authenticated launch nonce, process instance binding, unique request IDs, and
  strictly increasing sequences;
- monotonic cancellation generations and one in-flight generation at a time;
- defense-in-depth full GGUF length/SHA-256 verification before load;
- a hidden `llama-server.exe` child bound only to `127.0.0.1`, with a random
  per-launch API key, `--offline`, disabled Web UI, local model path, one slot,
  bounded context/output, and no built-in tools;
- Windows Job Object supervision with kill-on-close and a one-process limit;
- OpenAI-compatible SSE token streaming, bounded JSON Schema structured output,
  health, cancellation, unload, restart, and sanitized errors;
- no prompts, model output, API key, or credential material in diagnostics.

The app supervisor must also apply its normal deny-egress process policy. The
worker's `--offline` and local-only request construction are defense in depth,
not a replacement for that OS policy.

## GPU qualification guard

`qualify` refuses the Vulkan backend unless `--gpu-lock-file` currently contains
exactly `yes`. It never changes that file itself. The caller owns the repository's
coordination protocol and must restore the file to `no` after the bounded run.
The command captures `nvidia-smi` before load, while loaded, and after unload;
process RAM; load/reload time; TTFT; output token throughput; inter-token
p50/p95/p99; structured self-test; cancellation; and unload/reload behavior.

The resulting `npc.local-llm.measurement/v1` document is explicitly
`admissible_for_resource_governor: false`. It is unsigned and only one run. A
production `QualifiedResourceEnvelopeV1` still needs the model manager's minimum
20 samples, exact manifest/device/runtime/backend binding, validity window,
monotonic sequence, and trusted signature threshold.

## Bounded Vulkan qualification (2026-08-30)

The pinned stack passed its real local-review run on an NVIDIA GeForce RTX 4080
Laptop GPU (12,282 MiB reported by `nvidia-smi`). The evidence document is
`out/evidence/local-llm-qwen3-vulkan-2026-08-30.json`, SHA-256
`7fb027613c5d0524b1ced46cd11a3909df58775b63ee44f4486848ad767cc363`.
That run was bound to manifest SHA-256
`4cc58867ed65a53789d49820abaa23d9a25f03d64fdef4d86996e9497ffb60a0`.
The repository's concurrent v2 schema migration then changed metadata only; the
current v2 manifest SHA-256 is
`b17aa18054478eb8394d2d98f38e3411340bd0673a54b278c7ed124f510c0ed9`.
All artifact hashes, sizes, runtime identity, and self-test identity are
unchanged, but the prior run must still remain non-admissible for the new digest.

- load 17,471.928 ms; unload/reload 8,572.354 ms;
- time to first token 7,799.840 ms and 18.4712 output tokens/second;
- 62 inter-token samples: p50 55.754 ms, p95 80.397 ms, p99 262.554 ms;
- process working set 2,697,789,440 bytes, peak working set 3,698,737,152
  bytes, and private bytes 4,050,124,800;
- GPU used memory 438 MiB before, 4,061 MiB loaded, and 437 MiB after the
  final unload (3,623 MiB measured resident delta);
- constrained structured JSON/digest, cancellation, preserved-runtime reuse,
  unload, reload, artifact checksum, and license checks all passed.

This is one raw sample, not a production p99 envelope. The resource-envelope
aggregator must compute `p99_reload_millis` (as well as load and operation p99)
from at least 20 signed samples; it must not relabel this single reload value as
admissible evidence or rebind it to the migrated v2 manifest.

See [docs/INTEGRATION_HANDOFF.md](docs/INTEGRATION_HANDOFF.md) for the exact Rust,
Tauri, and UI integration contract and [docs/RESEARCH_LEDGER.md](docs/RESEARCH_LEDGER.md)
for the evidence behind the stack choice.

The separately pinned, non-production CUDA 12.4 comparison lane and its
20+20-sample ABBA/BAAB harness are documented in
[benchmark/README.md](benchmark/README.md). It is not a new production backend
and cannot run without an explicit GPU grant.
