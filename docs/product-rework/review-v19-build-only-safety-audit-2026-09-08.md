# Review v19 build-only safety audit

Date: 2026-09-08

Scope: static inspection of `scripts/windows/prepare-portable-review.ps1` and every helper it calls while preparing and verifying a portable review directory. This audit supports a build-only local review artifact. It is not live application, capture, game, audio, overlay, or mouth-motion qualification.

## Result

The preparation and verification chain does not invoke any produced Interactive NPCs executable. It does not start the control app, media broker, mouth worker, subtitle presenter, synthetic test game, WebView2 installer, screen capture, audio playback, or a native Windows smoke test.

The chain invokes these hidden tools for build or static inspection only:

- Cargo metadata and builds.
- CMake configure and builds.
- TypeScript, Vite, and Tauri `build --no-bundle` through pinned pnpm tooling.
- Python source, license, SBOM, provenance, and secret scanners.
- Node, Rust, Cargo, CMake, and Visual Studio version or metadata probes.
- PowerShell verification in a fresh hidden process.

All native product components are configured with `BUILD_TESTING=OFF` and their project-specific test switches disabled. The media broker also sets `NPC_MEDIA_BROKER_REGISTER_INTERACTIVE_WINDOWS_TESTS=OFF`. The CMake cache compatibility check refreshes an older cache when that value differs, so stale registration cannot carry into this build path. No project CMake pre-build, post-build, or custom command launches a product binary.

The portable verifier reads, hashes, and parses files. PE checks parse headers and imports from bytes rather than starting executables. Test-game verification validates the closed-world manifest, hashes, provenance, and media metrics without launching the fixture. The pinned WebView2 installer is checked by size, hash, version metadata, and Authenticode signature without being executed.

## Deliberately skipped

- Control application launch.
- Synthetic or commercial game launch.
- Packaged process join.
- Native Windows smoke executables.
- Window, display, overlay, WGC, or desktop capture tests.
- Physical or synthesized audio playback.
- Live-game or ordinary-NPC qualification.

These skips must remain visible in the final review receipt. A successful v19 build proves only that the frozen source produced a closed-world, hash-bound local review directory and passed its static verification gates.
