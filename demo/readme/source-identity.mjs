import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

const EXCLUDED_SEGMENTS = new Set(['node_modules', 'dist', 'artifacts', '.secrets', '.render-work']);
const SOURCE_SCOPES = ['demo/readme', 'apps/control/src/styles.css'];

function git(repoRoot, args, { allowFailure = false } = {}) {
  try {
    return execFileSync('git', args, { cwd: repoRoot, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch (error) {
    if (allowFailure) return '';
    throw error;
  }
}

function included(relativePath) {
  return !relativePath.split(/[\\/]/).some((segment) => EXCLUDED_SEGMENTS.has(segment));
}

function collectFiles(repoRoot, relativePath, files) {
  const absolute = path.join(repoRoot, relativePath);
  const stats = statSync(absolute);
  if (stats.isFile()) {
    if (included(relativePath)) files.push(relativePath.replaceAll('\\', '/'));
    return;
  }
  for (const entry of readdirSync(absolute, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const child = path.join(relativePath, entry.name);
    if (!included(child)) continue;
    if (entry.isDirectory() || entry.isFile()) collectFiles(repoRoot, child, files);
  }
}

export function deterministicSourceDigest(repoRoot) {
  const files = [];
  for (const scope of SOURCE_SCOPES) collectFiles(repoRoot, scope, files);
  files.sort((a, b) => a.localeCompare(b));
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
    scopes: SOURCE_SCOPES,
    excludedSegments: [...EXCLUDED_SEGMENTS].sort(),
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
