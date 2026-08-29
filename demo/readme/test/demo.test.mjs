import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createScene } from '../scene-config.mjs';
import { DEMO_SPEC } from '../storyboard.mjs';
import {
  createSourceIdentity,
  RENDER_INPUT_PATHS,
  verifyPortableEvidenceBinding,
} from '../source-identity.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const demoRoot = path.resolve(here, '..');

test('locks the approved deterministic publication contract', () => {
  assert.deepEqual(DEMO_SPEC.output, { width: 960, fps: 15, colors: 256 });
  assert.equal(DEMO_SPEC.openingAnchorHoldSeconds, 0.1);
  assert.equal(DEMO_SPEC.minimumCycleSeconds, 27);
  assert.equal(DEMO_SPEC.presentationMinimumSeconds, 28);
  assert.equal(DEMO_SPEC.presentationMaximumSeconds, 32);
});

test('source identity binds Git, effective render inputs, and the used lockfile', () => {
  const repoRoot = path.resolve(demoRoot, '..', '..');
  const identity = createSourceIdentity(repoRoot);
  assert.match(identity.git.headCommit, /^[0-9a-f]{40}$/);
  assert.equal(typeof identity.git.branch, 'string');
  assert.equal(typeof identity.git.dirty, 'boolean');
  assert.match(identity.sourceDigest.sha256, /^[0-9a-f]{64}$/);
  assert.ok(identity.sourceDigest.files.includes('demo/readme/scene/styles.css'));
  assert.deepEqual(identity.sourceDigest.files, RENDER_INPUT_PATHS);
  assert.ok(!identity.sourceDigest.files.includes('demo/readme/source-identity.mjs'));
  assert.ok(!identity.sourceDigest.files.includes('demo/readme/package.json'));
  assert.ok(!identity.sourceDigest.files.includes('apps/control/src/styles.css'));
  assert.ok(identity.sourceDigest.files.every((file) => !/(?:node_modules|dist|artifacts|\.secrets|\.render-work)/.test(file)));
  assert.equal(identity.packageLocks.demoPackageLock.used, true);
  assert.match(identity.packageLocks.demoPackageLock.sha256, /^[0-9a-f]{64}$/);
  assert.deepEqual(identity.packageLocks.rootPnpmLock, { path: 'pnpm-lock.yaml', used: false, sha256: null });
});

test('portable evidence follows content across a sanitized Git history', () => {
  const repoRoot = path.resolve(demoRoot, '..', '..');
  const identity = createSourceIdentity(repoRoot);
  const approved = {
    schemaVersion: 1,
    contract: 'portable-content-addressed-local-review-v1',
    renderInputs: identity.sourceDigest,
    packageLocks: identity.packageLocks,
    renderManifest: { path: 'docs/assets/demo/render-manifest.json', sha256: 'a'.repeat(64) },
    provenance: { path: 'docs/assets/demo/PROVENANCE.md', sha256: 'b'.repeat(64) },
  };

  const relocatedIdentity = {
    ...identity,
    git: { headCommit: 'f'.repeat(40), branch: 'local-rc-sanitized', dirty: false },
  };
  assert.doesNotThrow(() => verifyPortableEvidenceBinding({
    approved,
    currentIdentity: relocatedIdentity,
    renderManifestSha256: 'a'.repeat(64),
    provenanceSha256: 'b'.repeat(64),
  }));

  assert.throws(() => verifyPortableEvidenceBinding({
    approved,
    currentIdentity: {
      ...relocatedIdentity,
      sourceDigest: { ...relocatedIdentity.sourceDigest, sha256: '0'.repeat(64) },
    },
    renderManifestSha256: 'a'.repeat(64),
    provenanceSha256: 'b'.repeat(64),
  }), /render-input digest/);

  assert.throws(() => verifyPortableEvidenceBinding({
    approved,
    currentIdentity: relocatedIdentity,
    renderManifestSha256: '0'.repeat(64),
    provenanceSha256: 'b'.repeat(64),
  }), /render-manifest digest/);

  assert.throws(() => verifyPortableEvidenceBinding({
    approved,
    currentIdentity: {
      ...relocatedIdentity,
      packageLocks: {
        ...relocatedIdentity.packageLocks,
        demoPackageLock: {
          ...relocatedIdentity.packageLocks.demoPackageLock,
          sha256: '0'.repeat(64),
        },
      },
    },
    renderManifestSha256: 'a'.repeat(64),
    provenanceSha256: 'b'.repeat(64),
  }), /package-lock binding/);
});

test('scene is self-contained and labels simulated numbers honestly', () => {
  const html = readFileSync(path.join(demoRoot, 'scene', 'index.html'), 'utf8');
  const css = readFileSync(path.join(demoRoot, 'scene', 'styles.css'), 'utf8');
  const js = readFileSync(path.join(demoRoot, 'scene', 'app.js'), 'utf8');
  assert.match(html, /Eclipse Harbor/);
  assert.match(html, /Mara Venn/);
  assert.match(html, /ILLUSTRATIVE · SIMULATED RUN/);
  assert.match(html, /ORIGINAL SIMULATION/);
  assert.doesNotMatch(`${html}\n${css}\n${js}`, /https?:\/\//);
  assert.doesNotMatch(html, /<(?:img|audio|video)\b/i);
});

test('timeline contains every storyboard cue and the exact capture settings', async () => {
  const { config, plannedSeconds } = await createScene({
    url: 'http://127.0.0.1:4173/',
    out: 'staging/demo.gif',
    reviewDir: 'staging/review',
    logLevel: 'silent',
  });
  assert.equal(config.compose, 'overlay');
  assert.deepEqual(config.capture, { mode: 'deterministic', format: 'png', frameTimeoutMs: 20_000 });
  assert.deepEqual(config.loop, { strategy: 'anchor', minCycleSeconds: 27 });
  assert.equal(config.encode.width, 960);
  assert.equal(config.encode.fps, 15);
  assert.equal(config.encode.speed, 1.15);
  assert.equal(config.encode.colors, 256);
  assert.equal(config.encode.dither, 'none');
  assert.equal(config.encode.palette, 'full');
  assert.deepEqual(config.alsoEmit, ['webp', 'mp4']);
  assert.deepEqual(config.timeline.cues, DEMO_SPEC.cues);
  assert.ok(plannedSeconds >= DEMO_SPEC.expectedMinimumSeconds, `planned ${plannedSeconds}s`);
  assert.ok(plannedSeconds <= DEMO_SPEC.expectedMaximumSeconds, `planned ${plannedSeconds}s`);

  const loopIndex = config.timeline.steps.findIndex((step) => step.kind === 'loopAnchor');
  assert.ok(loopIndex >= 0);
  assert.deepEqual(config.timeline.steps[loopIndex + 1], { kind: 'hold', ms: 100 });
  const firstClickAfterAnchor = config.timeline.steps.findIndex((step, index) => index > loopIndex && step.kind === 'click');
  const preActionHoldMs = config.timeline.steps
    .slice(loopIndex + 1, firstClickAfterAnchor)
    .filter((step) => step.kind === 'hold')
    .reduce((total, step) => total + step.ms, 0);
  assert.equal(preActionHoldMs, 100);
});
