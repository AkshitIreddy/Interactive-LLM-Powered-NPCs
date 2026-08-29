# README demo renderer

This folder contains the one-target, deterministic Gifsmith 0.3.5 production for the project README. It renders an entirely original fictional game, **Eclipse Harbor**, and an original NPC, **Mara Venn**. No game capture, performer likeness, copied voice, logo, or third-party art is used. Its Windows-only cybernetic UI mirrors the control app’s final screenshot-informed language: wine-black fields, coral structure, cyan live/selected/action states, and off-white copy—without reproducing any game interface asset.

The render is intentionally independent of live app services and machine timing. Gifsmith advances Chromium virtual time one frame at a time, so a quiet/low-power computer only takes longer to produce the same 15 fps output.

## Commands

Run from this directory with Node 20 or newer:

```powershell
npm ci
npm test
npm run dry-run
npm run contact-sheet
npm run render
npm run refresh-evidence
npm run verify
```

`npm run render` writes all encodes to a staging directory, validates them, and only then transactionally promotes the GIF, animated WebP, MP4 review copy, poster, contact sheet, and manifest into `docs/assets/demo/`. Existing approved outputs are restored if promotion fails.

`npm run refresh-evidence` does not capture, encode, or replace media. It records the current Git context in the render manifest and updates `approved-evidence.json`, the portable content-addressed binding between the approved manifest, provenance statement, exact renderer lockfile, and the files that can affect the deterministic render. Run it only as an explicit evidence-authoring step; ordinary tests and packaging never rewrite checked-in evidence.

The Git commit, branch, and dirty state inside `render-manifest.json` describe the checkout used when that local-review manifest was authored. They are deliberately not a relocation lock: a clean sanitized release source has a different commit history even when every byte is identical. `npm run verify` instead checks the portable SHA-256 evidence contract, all media and supplemental hashes, media format constraints, and the recorded provenance shape. The release packager independently binds the complete current Git commit and full source-candidate digest before and after compilation, so moving the same content into a clean sanitized history does not weaken release provenance.

The production contract is locked in `storyboard.mjs`: 1440×900 capture, 960 px output, 15 fps, 1.15× scene speed, PNG frames, a 256-colour full GIF palette with no dithering, an opening anchor hold of 0.1 seconds, and a forward anchor loop of at least 27 seconds.

### Dependency audit note

As of 2026-08-28, `npm audit` reports GHSA-jmr9-qjv8-65gv in `extract-zip`, reached through Gifsmith’s `puppeteer-core` dependency, with no upstream fix available. This renderer never downloads or extracts a browser archive: Gifsmith is pointed at an already-installed Chrome/Edge binary in an isolated browser profile. Keep the package confined to this offline development tool, do not feed it untrusted browser archives, and refresh the lockfile when the upstream dependency is patched.

## Story beats

1. Response Console is open on its calm Home state.
2. The viewer selects the fictional Eclipse Harbor profile.
3. A deterministic compatibility scan completes.
4. Start opens an original simulated harbor scene.
5. Push-to-talk captures the player line: “Mara, did the north beacon answer?”
6. The Response Spine advances through the real product stages.
7. Mara Venn replies while the mouth rig moves within its anchored face region.
8. The session stops and closes back to the exact opening state for a clean forward loop.

The performance HUD is visibly labelled **ILLUSTRATIVE · SIMULATED RUN**. Its values are art direction, not benchmark claims.
