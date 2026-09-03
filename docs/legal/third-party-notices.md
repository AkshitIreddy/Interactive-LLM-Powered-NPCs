# Third-party notices and distribution boundaries

This notice is part of the local review candidate. It is an engineering provenance record, not legal advice and not a claim that a public release is cleared. The exact machine-readable classifications are in `legal/distribution-components.json`; the exact locked source inventory is in `legal/lockfiles.cdx.json`. File hashes in the final extracted-distribution manifest are authoritative for the bytes actually shipped.

## Project-owned material

Interactive LLM Powered NPCs project source, built executables, catalog metadata, compatibility profiles, generated application icon, subtitle style metadata, and the synthetic review target are distributed under the MIT license in `legal/LICENSE.txt`, except where an item carries its own provenance or third-party terms. Game and provider names are nominative compatibility references. No game executable, texture, character image, voice, save data, or other publisher asset is included.

## Locked application dependencies

The source candidate has four authoritative lock inputs: root Rust `Cargo.lock`, Tauri-control `Cargo.lock`, root `pnpm-lock.yaml`, and the README-demo `package-lock.json`. Strict validation requires a manually reviewed SPDX expression for every merged purl and exact equality between the lock inventory and license ledger. The CycloneDX file includes the full lock universe, source-lock origin, checksums supplied by the lock, and distribution classifications. Build-only and development-only entries in that file are not necessarily installed files.

The approved permissive license families present in the locked source universe include 0BSD, Apache-2.0 (including the LLVM exception where declared), BSD-2-Clause, BSD-3-Clause, BSL-1.0, CC0-1.0, CDLA-Permissive-2.0, ISC, MIT, MIT-0, MPL-2.0, Unicode-3.0, Unlicense, and Zlib. CC-BY entries are attribution-bearing data packages. An LGPL alternative is not a blanket approval: the affected expression also offers an approved permissive branch. The distribution manifest, not this paragraph, decides whether a component is in a payload.

The installed legal directory must contain this notice, the project MIT license, the distribution ledger, the artifact scope, the generated CycloneDX SBOM, and `legal/packages/THIRD-PARTY-LICENSE-FILES.json` plus every exact package license/copying/notice body named by that index. A package missing any one of those files is unexplained and fails reconciliation.

`uuid-simd@0.8.0` and `vsimd@0.8.0` declare MIT but their published crate payloads omit the repository-root `../../LICENSE` referenced by their READMEs. `legal/licenses/Nugine-simd-d74c030-MIT.txt` is the exact license body reviewed from pinned upstream commit `d74c030d9dc4f3cae02146d1f497ff62726ef09a`, with one documented final-LF normalization. `legal/license-material-overrides.json` binds the upstream and vendored hashes plus each crate's package-metadata linkage; this is not a general fallback mechanism.

Ten additional runtime crates also omit the legal bodies needed by the artifact collector. `alloc-stdlib@0.2.4` is bound to Dropbox's exact BSD-3-Clause repository-root license at commit `ae42d22078b98549e987d2f03d12df7b984fde47`. Five UNIC 0.9.0 crates are bound through their packaged `.cargo_vcs_info.json` records to commits `5878605364af97a3358368a6eaef02104af2e016` or `8a6ce83063d90b91ae2ce59eddb803edd393fca9`; the upstream MIT, Apache-2.0, AUTHORS, and copyright files are all installed. The copyright body differs only by three recorded relative Markdown-target rewrites so its links resolve to those exact prefixed companion files after collection; strict validation reverses those rewrites and reproduces the pinned upstream SHA-256. `webview2-com@0.38.2`, `webview2-com-sys@0.38.2`, and `webview2-com-macros@0.8.1` are bound to their exact `webview2-rs` revisions and repository-root MIT body. Each mapping verifies the crate's `Cargo.toml.orig`, `.cargo_vcs_info.json`, source revision, repository path, optional README, and every copied legal-body hash before collection.

`selectors@0.36.1` declares MPL-2.0 in `Cargo.toml.orig` at pinned Servo revision `635e1a19d02960588a00e189bd4bd5bdb150ec3d`, but neither the published crate nor that pinned tree contains a license body. Its exact component-scoped override installs Mozilla's official MPL-2.0 text from <https://www.mozilla.org/media/MPL/2.0/index.815ca599c9df.txt>; it is not reusable for packages with another declaration. Source form for the covered crate remains obtainable from the locked crate payload and <https://github.com/servo/stylo/tree/635e1a19d02960588a00e189bd4bd5bdb150ec3d/selectors>. Any public distribution must preserve the MPL notices and source-availability obligations.

## SQLite

The Rust runtime uses `rusqlite`/`libsqlite3-sys` with bundled SQLite 3.46.0. SQLite code is dedicated to the public domain; see <https://www.sqlite.org/copyright.html>. The SQLite code is statically embedded, so it is represented as a separate component whose bytes are covered by the containing executable hash rather than as a loose DLL.

## NVIDIA Riva API definitions

The NVIDIA Riva r2.17 protobuf definitions are pinned to upstream commit `a7d342c` and compiled into project code. They are MIT licensed. The exact vendored license is installed as `legal/licenses/NVIDIA-RIVA-PROTO-MIT.txt`; upstream provenance remains in `crates/providers-tts/proto/UPSTREAM.md`. NVIDIA hosted services, NIM containers, models, and SDK binaries are not granted or redistributed by that MIT proto license.

## Tauri, NSIS, WebView2, and Microsoft runtimes

The control application uses Tauri and an NSIS installer. Rust/JavaScript wrapper packages are covered by the lock ledger, but installer stubs and native NSIS/Tauri plugins are separate redistributed binaries. `legal/installer-toolchain-provenance.json` pins Tauri CLI 2.11.4, its Windows native npm binary, the exact NSIS 3.11 binary and corresponding-source archives, the NSIS files selected by Tauri, and `nsis-tauri-utils` 0.5.3 at commit `13d9edd27b69310e108d6fbd49f90992f8a05390`. The complete NSIS 3.11 zlib/libpng, bzip2, and CPL-1.0-with-LZMA-exception terms are installed as `legal/licenses/NSIS-3.11-COPYING.txt`; the exact `nsis-tauri-utils` Apache-2.0 and MIT bodies are installed beside it. The generated installer and `uninstall.exe` still require independent hashes in the final extracted-distribution manifest.

The release configuration sets Tauri's WebView mode to `skip` and uses the reviewed `customPinnedOfflineInstaller` NSIS hook, because locked Tauri's nominal `offlineInstaller` path performs a mutable network HEAD before consulting its cache. The custom hook embeds the reviewed x64 Evergreen Standalone Installer without package-time network access. It is Microsoft version 1.3.263.3, 258,438,352 bytes, SHA-256 `987a9d8b3107e84f9b53b4a077d28ae4814fc3d964d5a55c559e7334bbf24d61`, with a valid Microsoft Authenticode signature. Microsoft's official WebView2 distribution guidance expressly documents packaging the Evergreen Standalone Installer with an application for offline deployment. The isolated package run must fail unless its canonical cache contains those exact reviewed bytes, signature, and version, and the final manifest must record the embedded installer. Microsoft proprietary terms continue to govern the standalone installer and installed Evergreen Runtime; the separately serviced runtime on the user's system is not a project-owned lockfile library.

Release binaries may depend on the Microsoft Visual C++ release runtime according to Microsoft redistributable terms. Debug CRT imports (`MSVCP140D`, `VCRUNTIME140D`, `VCRUNTIME140_1D`, or `ucrtbased`) are forbidden in every shipping executable or DLL. A notice cannot cure a Debug CRT build; reconciliation blocks the artifact.

## Hosted APIs and optional local packs

OpenAI, Anthropic, Google Gemini, Groq, Cohere, NVIDIA NIM/Riva, Deepgram, AssemblyAI, ElevenLabs, Cartesia, Inworld, and configurable compatible endpoints are remote services. Provider models, containers, voices, and server implementations are not included. Users supply credentials and remain subject to each provider's account, model, content, and usage terms.

Local STT, TTS, LLM, embedding, and visual entries in the catalog are optional user-selected downloads, not base-installer payloads. Each must independently pin source revision, immutable artifact URL, size, SHA-256, license, extracted-file allowlist, runtime dependency, resource envelope, and self-test before activation. A catalog entry is not redistribution permission.

The OpenSeeFace MNV3/LM1 record covers two model files and the upstream license at commit `85aa70fc67582d046e771ea73625182a0d8f7475`; the pinned upstream README explicitly states that its code and models are BSD-2-Clause. Referenced training datasets and the upstream author's nonredistributable additional training data are not included. This remains an experimental face/landmark signal, not identity recognition or a complete lip-sync model. ONNX Runtime 1.22.1 is a separately governed optional runtime and is not bundled in the base installer. Its official Windows x64 archive, extracted-file allowlist, MIT license, and `ThirdPartyNotices.txt` are hash-pinned; the archive's bundled third-party terms remain applicable in addition to the ONNX Runtime project's MIT license. Optional activation remains blocked pending release-catalog trust, signed current-device evidence, whole-loadout admission, extracted optional-pack reconciliation, and human legal review.

The Kokoro/sherpa-onnx candidate includes a separately disclosed GPL-3.0-or-later eSpeak-NG component. It is not in the base installer. Any future project mirror or redistribution requires a complete copyleft review, corresponding-source compliance, and notices; the candidate manifest does not itself approve redistribution.

The BGE small English embedding candidate records the immutable upstream MIT model-card revision, but its Windows Python/ONNX runtime bundle is not approved for redistribution until every transitive wheel, native DLL, hash, and license body is locked. The OpenCV YuNet/SFace entry is private-evaluation-only: YuNet's MIT record does not resolve the pretrained SFace weight and training-data rights, so SFace redistribution, commercial use, production admission, and automatic download remain prohibited. These blocked catalog records are safety boundaries, not bundled payloads or implied grants.

## Fonts and subtitles

No font binary is bundled or downloaded. Subtitle manifests are project-owned MIT metadata. Windows fonts are resolved from the user's licensed system through platform APIs. Optional Noto family names are lookup hints only; if no installed font satisfies a role, the platform fallback is used. `assets/subtitles/licenses.v1.json` is the canonical no-bundled-font ledger.

## Demo tooling, FFmpeg wrappers, and native binaries

The README renderer is development-only. `ffmpeg-static` is GPL-3.0-or-later. The MIT license of the `ffprobe-static` npm wrapper applies to the wrapper package and does **not** determine the license of its embedded `ffprobe.exe`. Likewise, a PATH-selected `ffmpeg.exe` is a separate native payload whose enabled codecs and license depend on its build configuration.

No FFmpeg or ffprobe executable, wrapper payload, media codec binary, or generated demo tool is allowed in the base installer or synthetic review test game. If that rule ever changes, the exact executable hash, `-version` build configuration, SPDX conclusion, copyright notices, license text, and corresponding source for that exact binary must be recorded before copying it.

## Synthetic review test game

The prepared test game is a project-authored compatibility fixture. Its directory is closed-world: only the executable, `REVIEW-FIXTURE-MANIFEST.json`, `review-test-game.cdx.json`, `THIRD-PARTY-NOTICES.md`, and `README.md` may exist. The fixture manifest hashes and classifies every file except itself, with an explicit circular-self-hash exclusion. It declares no third-party binaries and uses only Windows inbox dependencies. Extra, missing, symlinked, hash-mismatched, Debug-CRT, or unknown-component files fail reconciliation.

## Release blockers that this notice does not waive

- Reconcile the generated NSIS installer and installed `uninstall.exe` hashes against the reviewed Tauri/NSIS input provenance; reviewed tool inputs do not predict generated output bytes.
- Verify that the `customPinnedOfflineInstaller` hook embeds the exact reviewed x64 Microsoft WebView2 Evergreen Standalone Installer SHA-256/version/signature from the isolated cache, with Tauri mode `skip` and no package-time network acquisition.
- Produce and reconcile the final package file manifest; source-lock completeness alone does not prove which native or generated bytes shipped.
- Build every shipping native executable in Release mode and pass the Debug CRT import gate. The currently staged media-broker binary must not be accepted until replaced and re-audited.
- Install the generated CycloneDX SBOM and all required legal resources inside the application, then verify their extracted hashes.
- Run the exact-license-material collector for the final Windows normal dependency graph and reconcile every indexed body in the extracted installer. The 964-component merged source ledger is complete at the current working snapshot but intentionally broader than the runtime payload.
- Keep ONNX Runtime/OpenSeeFace and every other optional pack inactive until its own runtime and extracted payload are fully admitted.
- Obtain human legal review before any public distribution, especially for optional model/data terms and any future GPL-containing payload.
