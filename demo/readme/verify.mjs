import path from 'node:path';
import { fileURLToPath } from 'node:url';
import ffprobeStatic from 'ffprobe-static';
import { readFileSync, statSync } from 'node:fs';
import { sha256, verifyMediaSet } from './verify-lib.mjs';
import { createSourceIdentity, verifyPortableEvidenceBinding } from './source-identity.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const publicRoot = path.resolve(here, '..', '..', 'docs', 'assets', 'demo');
const repoRoot = path.resolve(here, '..', '..');
const report = verifyMediaSet(publicRoot, ffprobeStatic.path);
const manifestPath = path.join(publicRoot, 'render-manifest.json');
const provenancePath = path.join(publicRoot, 'PROVENANCE.md');
const approvedPath = path.join(here, 'approved-evidence.json');
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
const approved = JSON.parse(readFileSync(approvedPath, 'utf8'));
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
if (!/^[0-9a-f]{40}$/.test(recordedIdentity.git?.headCommit ?? '')) throw new Error('recorded source identity has an invalid Git commit');
if (typeof recordedIdentity.git?.branch !== 'string' || recordedIdentity.git.branch.length === 0) throw new Error('recorded source identity has an invalid Git branch');
if (typeof recordedIdentity.git?.dirty !== 'boolean') throw new Error('recorded source identity has an invalid Git dirty state');
if (!/^[0-9a-f]{64}$/.test(recordedIdentity.sourceDigest?.sha256 ?? '')) throw new Error('recorded source identity has an invalid scoped digest');
if (recordedIdentity.packageLocks?.rootPnpmLock?.used !== false || recordedIdentity.packageLocks?.rootPnpmLock?.sha256 !== null) throw new Error('recorded root pnpm lock must be explicitly marked unused');
if (manifest.evidence.classification !== 'mutable-local-review' || manifest.evidence.immutableRcEvidence !== false) throw new Error('evidence must be classified as mutable local-review, not immutable RC evidence');
if (!manifest.notes?.some((note) => note.includes('mutable local-review evidence'))) throw new Error('manifest must state the dirty-source evidence limitation');

verifyPortableEvidenceBinding({
  approved,
  currentIdentity: identity,
  renderManifestSha256: sha256(manifestPath),
  provenanceSha256: sha256(provenancePath),
});

console.log(JSON.stringify({
  ok: true,
  root: publicRoot,
  media: report,
  evidence: {
    classification: manifest.evidence.classification,
    immutableRcEvidence: manifest.evidence.immutableRcEvidence,
    recordedRenderContext: recordedIdentity.git,
    approvedContract: approved.contract,
    currentSourceIdentity: identity,
  },
}, null, 2));
