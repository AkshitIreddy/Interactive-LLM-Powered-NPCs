import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';

export const RENDER_INPUT_PATHS = Object.freeze([
  'demo/readme/finalize-staged.mjs',
  'demo/readme/package-lock.json',
  'demo/readme/render.mjs',
  'demo/readme/scene-config.mjs',
  'demo/readme/scene/app.js',
  'demo/readme/scene/index.html',
  'demo/readme/scene/styles.css',
  'demo/readme/storyboard.mjs',
  'demo/readme/verify-lib.mjs',
]);

function git(repoRoot, args, { allowFailure = false } = {}) {
  try {
    return execFileSync('git', args, { cwd: repoRoot, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch (error) {
    if (allowFailure) return '';
    throw error;
  }
}

function fail(message) {
  throw new Error(`README demo evidence: ${message}`);
}

function equalJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function verifyPortableEvidenceBinding({ approved, currentIdentity, renderManifestSha256, provenanceSha256 }) {
  if (approved?.schemaVersion !== 1 || approved?.contract !== 'portable-content-addressed-local-review-v1') {
    fail('unsupported approved-evidence contract');
  }
  if (approved.renderManifest?.path !== 'docs/assets/demo/render-manifest.json') {
    fail('approved render-manifest path is invalid');
  }
  if (approved.provenance?.path !== 'docs/assets/demo/PROVENANCE.md') {
    fail('approved provenance path is invalid');
  }
  if (approved.renderInputs?.algorithm !== currentIdentity.sourceDigest.algorithm) {
    fail('render-input digest algorithm changed');
  }
  if (approved.renderInputs?.sha256 !== currentIdentity.sourceDigest.sha256) {
    fail('render-input digest changed');
  }
  if (approved.renderInputs?.fileCount !== currentIdentity.sourceDigest.fileCount ||
      !equalJson(approved.renderInputs?.files, currentIdentity.sourceDigest.files)) {
    fail('render-input file set changed');
  }
  if (!equalJson(approved.packageLocks, currentIdentity.packageLocks)) {
    fail('renderer package-lock binding changed');
  }
  if (approved.renderManifest?.sha256 !== renderManifestSha256) {
    fail('render-manifest digest changed');
  }
  if (approved.provenance?.sha256 !== provenanceSha256) {
    fail('provenance digest changed');
  }
}

export function deterministicSourceDigest(repoRoot) {
  const files = [...RENDER_INPUT_PATHS];
  const digest = createHash('sha256');
  for (const relative of files) {
    digest.update(relative, 'utf8');
    digest.update(Buffer.from([0]));
    digest.update(readFileSync(path.join(repoRoot, relative)));
    digest.update(Buffer.from([0]));
  }
  return {
    algorithm: 'sha256:path-nul-bytes-nul:v1',
    sha256: digest.digest('hex'),
    fileCount: files.length,
    files,
  };
}

export function createSourceIdentity(repoRoot) {
  const branch = git(repoRoot, ['symbolic-ref', '--short', '-q', 'HEAD'], { allowFailure: true }) || 'DETACHED';
  const status = git(repoRoot, ['status', '--porcelain=v1', '--untracked-files=normal']);
  const demoLock = path.join(repoRoot, 'demo', 'readme', 'package-lock.json');
  return {
    git: {
      headCommit: git(repoRoot, ['rev-parse', 'HEAD']),
      branch,
      dirty: status.length > 0,
    },
    sourceDigest: deterministicSourceDigest(repoRoot),
    packageLocks: {
      demoPackageLock: {
        path: 'demo/readme/package-lock.json',
        used: true,
        sha256: createHash('sha256').update(readFileSync(demoLock)).digest('hex'),
      },
      rootPnpmLock: {
        path: 'pnpm-lock.yaml',
        used: false,
        sha256: null,
      },
    },
  };
}

export function evidenceClassification(sourceIdentity) {
  return {
    classification: 'mutable-local-review',
    immutableRcEvidence: false,
    reason: sourceIdentity.git.dirty
      ? 'The Git working tree is dirty. This evidence identifies mutable local source and is suitable for local review only; it is not immutable release-candidate evidence.'
      : 'This evidence has not been signed or bound to an immutable release artifact and remains local-review evidence.',
  };
}
