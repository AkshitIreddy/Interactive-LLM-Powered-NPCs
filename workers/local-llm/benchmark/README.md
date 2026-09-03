# Qwen3 b10689 direct-backend qualification lane

This directory is a benchmark-only lane for comparing the exact same Qwen3
GGUF on the official llama.cpp `b10689` Vulkan and CUDA 12.4 Windows builds. It
does not change the product default: hosted APIs remain the primary route, and
the optional local pack remains blocked until signed resource admission exists.

## Frozen provenance and storage

| Artifact | Upstream identity | Download bytes | SHA-256 |
|---|---|---:|---|
| CUDA 12.4 llama.cpp programs/DLLs | `b10689@57291f2644af8c9df0dd8d44395881c5bdcf0ecd` | 250,542,232 | `1c98e0c4eb7f8cbaec0349a57cf7ab2f3f8bfd41743e56bdd1d27e21b5738cf0` |
| NVIDIA CUDA 12.4 runtime DLLs from the same release | GitHub asset `536119339` | 391,443,627 | `8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6` |
| Existing Vulkan comparison archive | same commit and release | 34,905,903 | `3da600ff52a746d82e32a2ba3f0382e3bf782bd3a8fece661b646e1dee7ac1c6` |
| Existing Qwen3 Q4_K_M GGUF | exact pack artifact | 2,497,281,120 | `3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597` |

The two CUDA archives total **641,985,859 bytes** and expand to exactly **55
files / 1,158,588,189 bytes**. Extraction plus retained archives has a
1,800,574,048-byte logical peak before filesystem overhead. The locally
installed payload is anchored back to both pinned archive hashes on every full
verification; a mutable receipt alone is not trusted.

The installed CUDA runtime is at
`%LOCALAPPDATA%\InteractiveNPCs\BenchmarkArtifacts\llama-b10689-cuda12.4\installed`.
It is intentionally outside source control and does not contain model bytes.
The host's CUDA 12.1 toolkit is not changed. This lane uses the release's own
CUDA 12.4 runtime DLLs and relies on NVIDIA's documented CUDA 12.x driver minor
version compatibility.

## Safe preparation and verification

From the repository root, with Python configured not to write bytecode:

```powershell
$env:PYTHONDONTWRITEBYTECODE = '1'
$env:PYTHONPATH = 'workers/local-llm'
$artifactRoot = "$env:LOCALAPPDATA\InteractiveNPCs\BenchmarkArtifacts\llama-b10689-cuda12.4"

python -m benchmark --repo-root . plan

python -m benchmark --repo-root . verify-cuda `
  --archive-root $artifactRoot `
  --cuda-install-root "$artifactRoot\installed"

python -m unittest discover -s workers/local-llm/tests -v
```

`plan` does not read the GPU lock, start a model process, or access the network.
The full verifier hashes both archives, streams every ZIP member, compares all
55 installed file sizes/hashes, and verifies the receipt/release/commit.

## Direct run contract

The measured schedule is two warmups per backend followed by 20 Vulkan and 20
CUDA observations in alternating ABBA/BAAB blocks. Every direct observation
uses the same:

- GGUF SHA, embedded Jinja template, structured prompt, seed `424242`, and
  greedy sampling (`temperature=0`, `top_p=1`);
- `ctx=8192`, `batch=2048`, `ubatch=512`, `KV=f16/f16`, Flash Attention
  `auto`, 99 GPU layers, 8 generation/batch threads, one slot, no prompt cache;
- b10689 source commit, loopback-only offline server, hidden supervised Windows
  process, and no user-provided runtime flags.

Every sample measures a cold load, structured streaming response, cancellation,
same-process recovery, unload/VRAM return, reload, post-reload structured
recovery, final unload/VRAM return, RAM, TTFT, total operation time,
inter-token gaps, and tokens/second. Load, reload, operation, TTFT and throughput
p50/p95/p99 are emitted only after all 20 observations for that backend exist.
The raw report remains unsigned and non-admissible until the model manager wraps
it in the separately signed qualified-resource envelope.

The actual run is Windows-only and has two independent gates: the coordination
file must already contain `yes`, and `--execute-model-benchmark` must be present.
The harness only reads that file and never changes it. Root coordination must
grant this lane exclusive ownership immediately before running:

```powershell
$env:PYTHONDONTWRITEBYTECODE = '1'
$env:PYTHONPATH = 'workers/local-llm'
$artifactRoot = "$env:LOCALAPPDATA\InteractiveNPCs\BenchmarkArtifacts\llama-b10689-cuda12.4"

python -m benchmark --repo-root . run-direct `
  --local-review-install-root "$env:LOCALAPPDATA\InteractiveNPCs\ModelPacks" `
  --archive-root $artifactRoot `
  --cuda-install-root "$artifactRoot\installed" `
  --gpu-lock-file 'C:\Users\akshi\Desktop\Code Palace\gpu use.txt' `
  --report 'out\evidence\local-llm-qwen3-b10689-vulkan-cuda12.4-abba.json' `
  --execute-model-benchmark
```

All task-owned console processes use hidden/noninteractive launch and captured
stdout/stderr. Existing `llama-server.exe` processes make preflight fail, so the
results cannot be silently attributed across another model session.

## Game-load and LM Studio controls

The baseline/idle/active matrix is deliberately separate. It uses the
project-owned Eclipse Harbor synthetic replay at 960x600/1 fps muted and
1920x1080/60 fps muted, five repeats per backend/state. Those five observations
characterize bounded host contention only: they do not produce p99 and they do
not claim real-game FPS impact. Because this fixture opens a task-owned GUI, the
`game-matrix` command additionally requires `--allow-synthetic-game-gui` after
root coordinates the visible test; it prefers the secondary monitor and never
moves an unrelated window.

LM Studio is recorded only as a non-redistributable observational control:
LM Studio `0.4.13+1`, CLI commit `0b2a176`, engine
`llama.cpp-win-x86_64-nvidia-cuda12-avx2@2.31.2`, llama.cpp `b10662`, bundled
CUDA 12.8. Its exact installed CLI/engine/vendor DLL hashes are in
`lmstudio-control.2.31.2.json`. A dry-run proved that the exact GGUF can be
registered by symbolic link without copying the model. An actual control must
skip rather than copy if Windows denies that symbolic link, must not unload a
user-owned model, and is limited to 10 raw observations with no p99 or admission
claim because runtime revision and load controls differ.

After a separate exclusive GPU grant, the conditional control is invoked with:

```powershell
python -m benchmark --repo-root . lmstudio-control `
  --local-review-install-root "$env:LOCALAPPDATA\InteractiveNPCs\ModelPacks" `
  --gpu-lock-file 'C:\Users\akshi\Desktop\Code Palace\gpu use.txt' `
  --report 'out\evidence\local-llm-qwen3-lmstudio-cuda12.8-control.json' `
  --execute-model-benchmark
```

It first verifies the exact CLI, engine and vendor DLL hashes and refuses to
run if LM Studio already has a model or server active. It uses a temporary
symbolic link, unloads only its own identifier, stops only the server it starts,
and removes only the link it created. All CLI calls are hidden, noninteractive,
argv-based processes with captured output; no credential or profile content is
copied into evidence.
