import path from 'node:path';
import { fileURLToPath } from 'node:url';
import ffprobeStatic from 'ffprobe-static';
import { readFileSync, statSync } from 'node:fs';
import { sha256, verifyMediaSet } from './verify-lib.mjs';
import { createSourceIdentity } from './source-identity.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const publicRoot = path.resolve(here, '..', '..', 'docs', 'assets', 'demo');
const repoRoot = path.resolve(here, '..', '..');
const report = verifyMediaSet(publicRoot, ffprobeStatic.path);
const manifest = JSON.parse(readFileSync(path.join(publicRoot, 'render-manifest.json'), 'utf8'));
const identity = createSourceIdentity(repoRoot);

for (const [name, actual] of Object.entries(report)) {
  const recorded = manifest.media?.[name];
  if (!recorded) throw new Error(`render-manifest.json is missing media.${name}`);
  if (recorded.sha256 !== actual.sha256 || recorded.bytes !== actual.bytes) {
    throw new Error(`${name}: manifest hash/size does not match the current media`);
  }
}
for (const name of ['poster.png', 'contact-sheet.png', 'temporal-review-summary.md']) {
  const file = path.join(publicRoot, name);
  const recorded = manifest.supplemental?.[name];
  if (!recorded || recorded.sha256 !== sha256(file) || recorded.bytes !== statSync(file).size) {
    throw new Error(`${name}: manifest supplemental hash/size does not match`);
  }
}
const recordedIdentity = manifest.evidence?.sourceIdentity;
if (!recordedIdentity) throw new Error('render-manifest.json is missing evidence.sourceIdentity');
for (const key of ['headCommit', 'branch', 'dirty']) {
  if (recordedIdentity.git?.[key] !== identity.git[key]) throw new Error(`source identity Git ${key} changed since evidence refresh`);
}
if (recordedIdentity.sourceDigest?.sha256 !== identity.sourceDigest.sha256) throw new Error('scoped source digest changed since evidence refresh');
if (recordedIdentity.packageLocks?.demoPackageLock?.sha256 !== identity.packageLocks.demoPackageLock.sha256) throw new Error('demo package-lock hash changed since evidence refresh');
if (recordedIdentity.packageLocks?.rootPnpmLock?.used !== false || recordedIdentity.packageLocks?.rootPnpmLock?.sha256 !== null) throw new Error('root pnpm lock must be explicitly recorded as unused');
if (manifest.evidence.classification !== 'mutable-local-review' || manifest.evidence.immutableRcEvidence !== false) throw new Error('evidence must be classified as mutable local-review, not immutable RC evidence');
if (!manifest.notes?.some((note) => note.includes('mutable local-review evidence'))) throw new Error('manifest must state the dirty-source evidence limitation');

console.log(JSON.stringify({ ok: true, root: publicRoot, media: report, evidence: manifest.evidence }, null, 2));
