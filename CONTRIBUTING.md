# Contributing to Interactive LLM Powered NPCs 2.0

Thank you for helping with the 2.0 rewrite. It is a security- and latency-sensitive Windows application, not a continuation of the legacy notebook workflow.

## Before you begin

- Read the [roadmap](ROADMAP.md), [security policy](SECURITY.md), and [game-content policy](docs/legal/game-content-policy.md).
- Search existing issues and proposals before starting a large subsystem or game profile.
- Keep public claims aligned with [CHANGELOG.md](CHANGELOG.md). Planned capability is not implemented capability.
- Never commit credentials, transcripts, recordings, proprietary game captures, model weights, crash dumps, or personal data.
- Do not run the legacy code path that writes model output into Python and executes it.

## Development workflow

1. Use Windows 10 22H2 or Windows 11 x64 for native runtime, capture, overlay, packaging, and clean-machine validation. WSL is useful for inspection, but cannot certify Windows integration.
2. Follow [developer setup](docs/development/setup.md).
3. Create a focused branch and keep unrelated local changes intact.
4. Add or update tests, fixtures, documentation, privacy notes, and benchmark expectations with the change.
5. Run the relevant canonical commands:

   ```powershell
   ./dev.ps1 lint
   ./dev.ps1 test
   ```

6. For visual work, include rendered screenshots and a real interaction check. For performance work, attach raw benchmark output and its environment manifest.

The command dispatcher may report a component as unavailable while foundation work is still landing. That is a pre-release limitation, not permission to claim it passed.

## Change expectations

### Runtime

- Keep the state machine bounded and typed; make cancellation idempotent and late events harmless.
- Preserve session, turn, sequence, deadline, and cancellation-generation semantics.
- Never silently change provider, privacy boundary, or cost.
- Do not record prompt, transcript, audio, image, secret, or memory content in default telemetry.

### Frontend

- Keep continuous audio/video out of WebView IPC.
- Support keyboard use, Narrator-friendly labels, high contrast, reduced motion, and 100–200% scaling.
- Never display simulated metrics as measured results.
- Verify visual states from rendered screenshots; DOM assertions alone are insufficient.

### Providers and models

- Discover capabilities instead of assuming parity.
- Document transmitted data, privacy/region behavior, cancellation, codecs, limits, costs, and fallback compatibility.
- Pin revisions and record hashes, upstream source, license, and measured resource envelope.
- Research-only or non-commercial weights cannot enter the normal redistributable catalog.

### Game profiles

- Profiles are declarative and schema-validated; no executable Python or arbitrary commands.
- Use original writing or material with compatible documented rights. Do not copy wiki prose or extract game media.
- Declare detection, safety, capabilities, evidence, fallbacks, diagnostics, provenance, character defaults, and spoiler boundaries.
- Executable adapters are separate first-party packages, exact-build allowlisted, signed for release, and blocked in protected/ambiguous configurations.
- Add deterministic replay validation; live-game certification is recorded separately.

## Pull-request checklist

- The change is focused and reaches a working state.
- Tests cover behavior, failures, cancellation, offline operation, and migrations as relevant.
- Public interfaces and user-visible behavior are documented.
- No secret or copyright-incompatible content is present.
- Visual evidence is included for UI, overlay, animation, or rendering changes.
- Performance claims link to raw results; acceptance targets are not presented as results.
- Packaging changes do not publish, sign, upload, tag, or activate an update feed without explicit approval.

## Commit style

Use small working commits with Conventional Commit messages:

```text
feat(runtime): add late-event cancellation guard
fix(profiles): reject ambiguous online executable
docs(models): clarify local pack license gate
```

## Security and contributor rights

Report vulnerabilities privately using [SECURITY.md](SECURITY.md). By contributing, you confirm that you may submit the material under the project license and any declared compatible asset license. Contributions do not transfer rights to third-party games, characters, voices, or model weights.
