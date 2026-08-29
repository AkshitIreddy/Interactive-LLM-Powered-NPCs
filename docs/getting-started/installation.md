# Installation

## There is no public 2.0 installer yet

2.0 is a source-development preview. No download page, public RC, update feed, model catalog, or checksum list is active. Do not trust third-party binaries claiming otherwise.

The legacy Python/Jupyter/SadTalker instructions are not a supported installation method. In particular, do not place keys in `apikeys.json`, execute per-character `voice.py` files, load legacy pickles, or run model-generated `temp.py`.

## Developer source setup

Developers should use the existing repository root (the directory containing `.git`) and follow [source setup](../development/setup.md). The intended Windows entry point is:

```powershell
./dev.ps1 setup
./dev.ps1 dev
```

During foundation work, the dispatcher may accurately report that a component is not yet available. Check [CHANGELOG.md](../../CHANGELOG.md) rather than assuming every planned subsystem is runnable.

## Local release-candidate testing

When an RC is prepared, testers will receive all of the following together through an explicitly approved private channel:

- exact version/commit;
- installer filename, size, and SHA-256;
- signer identity or an explicit statement that the local test build is unsigned;
- clean Windows installation instructions;
- known limitations and rollback/removal instructions;
- diagnostic and acceptance-test checklist.

Do not disable Windows security warnings merely to run an unexpected build. Verify the digest and provenance first.

## Planned end-user installer behavior

The planned NSIS installer is non-admin where feasible, model-free, and independent of system Python/Node/Rust/CUDA/FFmpeg. After installation, onboarding configures hosted API routes. No model downloads automatically. A future qualified generic lip-sync pack may be offered only as a separate explicit choice with model, size, RAM/VRAM, license, quality, and experimental disclosures.

This section describes the release gate, not currently downloadable software.
