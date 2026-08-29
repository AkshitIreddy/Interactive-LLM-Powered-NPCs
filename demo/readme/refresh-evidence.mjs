import { existsSync, readFileSync, renameSync, statSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { sha256 } from './verify-lib.mjs';
import { createSourceIdentity, evidenceClassification } from './source-identity.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '..', '..');
const publicRoot = path.join(repoRoot, 'docs', 'assets', 'demo');
const manifestPath = path.join(publicRoot, 'render-manifest.json');
const provenancePath = path.join(publicRoot, 'PROVENANCE.md');
const approvedPath = path.join(here, 'approved-evidence.json');
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
const sourceIdentity = createSourceIdentity(repoRoot);

manifest.evidence = {
  schemaVersion: 1,
  refreshedAt: new Date().toISOString(),
  ...evidenceClassification(sourceIdentity),
  sourceIdentity,
};

manifest.supplemental = {};
for (const name of ['poster.png', 'contact-sheet.png', 'temporal-review-summary.md']) {
  const file = path.join(publicRoot, name);
  if (!existsSync(file)) throw new Error(`Cannot refresh evidence: ${name} is missing`);
  manifest.supplemental[name] = { bytes: statSync(file).size, sha256: sha256(file) };
}

const mutableNote = 'This manifest is mutable local-review evidence, not immutable release-candidate evidence; release packaging binds the current clean source independently.';
manifest.notes = [...new Set([...(manifest.notes ?? []), mutableNote])];

const incoming = `${manifestPath}.incoming`;
writeFileSync(incoming, `${JSON.stringify(manifest, null, 2)}\n`);
renameSync(incoming, manifestPath);

const approved = {
  schemaVersion: 1,
  contract: 'portable-content-addressed-local-review-v1',
  renderInputs: sourceIdentity.sourceDigest,
  packageLocks: sourceIdentity.packageLocks,
  renderManifest: {
    path: 'docs/assets/demo/render-manifest.json',
    sha256: sha256(manifestPath),
  },
  provenance: {
    path: 'docs/assets/demo/PROVENANCE.md',
    sha256: sha256(provenancePath),
  },
};
const approvedIncoming = `${approvedPath}.incoming`;
writeFileSync(approvedIncoming, `${JSON.stringify(approved, null, 2)}\n`);
renameSync(approvedIncoming, approvedPath);

console.log(JSON.stringify({ ok: true, manifest: manifestPath, approvedEvidence: approvedPath, evidence: manifest.evidence }, null, 2));
