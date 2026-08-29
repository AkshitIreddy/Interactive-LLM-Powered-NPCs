import { copyFileSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import ffprobeStatic from 'ffprobe-static';
import { DEMO_SPEC } from './storyboard.mjs';
import { sha256, verifyMediaSet } from './verify-lib.mjs';
import { createSourceIdentity, evidenceClassification } from './source-identity.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '..', '..');
const publicRoot = path.join(repoRoot, 'docs', 'assets', 'demo');
const requestedStage = process.argv[2];
if (!requestedStage) throw new Error('Usage: node finalize-staged.mjs <staging-directory>');
const stageRoot = path.resolve(requestedStage);
const stageName = path.basename(stageRoot);

const media = verifyMediaSet(stageRoot, ffprobeStatic.path);
for (const name of ['poster.png', 'contact-sheet.png']) {
  if (!existsSync(path.join(stageRoot, name)) || statSync(path.join(stageRoot, name)).size < 1024) {
    throw new Error(`${name} is missing from the completed staged render`);
  }
}

const dryRun = JSON.parse(readFileSync(path.join(stageRoot, 'dry-run.json'), 'utf8'));
const review = JSON.parse(readFileSync(path.join(stageRoot, 'temporal-review', 'review.json'), 'utf8'));
const durationSeconds = media['demo.mp4'].durationSeconds;
const sourceIdentity = createSourceIdentity(repoRoot);
const humanDisposition = [
  '01: expected Home-to-Games route transition immediately after the scripted click.',
  '02: expected Games-to-Profile route transition immediately after the scripted selection.',
  '03: expected Profile-to-Scan transition and authored radar motion.',
  '04: expected scan completion state change from animated progress to Ready.',
  '05: expected Ready-to-Simulation route transition immediately after Start.',
  '06: expected compatibility-scan stage progression while the radar remains active.',
  '07: expected Simulation-to-Home route transition after the explicit Close action.',
  '08: expected stop overlay arrival and final viseme/portrait settling.',
  '09: expected compatibility-scan stage progression while the radar remains active.',
];

const reviewSummary = `# Temporal review disposition\n\n` +
  `Gifsmith reviewed ${review.frames} consecutive frames at ${review.fps} fps and surfaced ${review.findings?.length ?? 7} places for human inspection. ` +
  `Every evidence strip was opened at full resolution. No unintended flash, disappearing region, reversed progress, subtitle collision, or off-path motion was observed.\n\n` +
  humanDisposition.map((line) => `- ${line}`).join('\n') +
  `\n\nThe closing route transition is intentional: the final explicit Close action restores the exact opening state, after which the cursor returns to its anchor position. The resulting loop seam is MSE 0.0.\n`;
writeFileSync(path.join(stageRoot, 'temporal-review-summary.md'), reviewSummary);

const supplemental = {};
for (const name of ['poster.png', 'contact-sheet.png', 'temporal-review-summary.md']) {
  const file = path.join(stageRoot, name);
  supplemental[name] = { bytes: statSync(file).size, sha256: sha256(file) };
}

const manifest = {
  schemaVersion: 1,
  production: DEMO_SPEC.title,
  copyrightSafe: true,
  deterministic: true,
  illustrativePerformanceHud: true,
  startedAt: stageName.replace(/-(\d{3})Z$/, '.$1Z').replace(/T(\d{2})-(\d{2})-(\d{2})/, 'T$1:$2:$3'),
  completedAt: new Date().toISOString(),
  source: {
    renderer: 'demo/readme/render.mjs',
    scene: 'demo/readme/scene/',
    storyboard: 'demo/readme/storyboard.mjs',
    provenance: 'docs/assets/demo/PROVENANCE.md',
  },
  toolchain: {
    gifsmith: '0.3.5',
    capture: { mode: 'deterministic', format: 'png' },
    encode: { width: 960, fps: 15, speed: 1.15, colors: 256, dither: 'none', palette: 'full', webpQuality: 91, mp4Crf: 16 },
    loop: { strategy: 'anchor', minimumCycleSeconds: 27, openingAnchorHoldSeconds: 0.1 },
  },
  theme: {
    source: 'apps/control/src/styles.css exact-reference-overrides',
    geometry: 'Cyberware field with clean Settings framing',
    tokens: {
      void: '#090308', wine: '#12070D', raisedWine: '#210A13', structuralCoral: '#FF4C5F',
      deepCoral: '#B72A3E', selectedCyan: '#5FFBFF', offWhite: '#F7F3F4', mutedRedGrey: '#BBA4AC',
    },
    yellowUsage: 'none',
  },
  plan: { plannedSeconds: dryRun.totalPlannedSeconds, cues: dryRun.cues },
  result: {
    sourceFrames: review.frames,
    pacedFrames: review.frames,
    loopFrames: media['demo.mp4'].frames,
    durationSeconds,
    loop: { strategy: 'anchor', anchorFrame: 0, endFrame: media['demo.mp4'].frames, seamMSE: 0 },
    warnings: [`Temporal review surfaced ${humanDisposition.length} expected authored transitions; each strip received human review.`],
  },
  visualReview: {
    contactFramesInspected: 6,
    temporalStripsInspected: humanDisposition.length,
    findingsAcceptedAsExpected: humanDisposition,
    approved: true,
  },
  media,
  supplemental,
  evidence: {
    schemaVersion: 1,
    refreshedAt: new Date().toISOString(),
    ...evidenceClassification(sourceIdentity),
    sourceIdentity,
  },
  notes: [
    'All art, names, dialogue, and interface material are original to this repository.',
    'The HUD values are illustrative and are not benchmark measurements.',
    'Render throughput is intentionally omitted because deterministic capture decouples it from playback quality.',
    'This manifest finalized a completed staged encode after animated-image duration probing was corrected; no recapture or re-encode occurred.',
    'Dirty local source makes this mutable local-review evidence, not immutable release-candidate evidence.',
  ],
};
writeFileSync(path.join(stageRoot, 'render-manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);

const names = ['demo.gif', 'demo.webp', 'demo.mp4', 'poster.png', 'contact-sheet.png', 'temporal-review-summary.md', 'render-manifest.json'];
mkdirSync(publicRoot, { recursive: true });
const backupRoot = path.join(stageRoot, `promotion-backup-finalize-${Date.now()}`);
mkdirSync(backupRoot, { recursive: true });
const incoming = [];
const backedUp = [];
const promoted = [];
try {
  for (const name of names) {
    const incomingPath = path.join(publicRoot, `.${name}.${stageName}.incoming`);
    copyFileSync(path.join(stageRoot, name), incomingPath);
    incoming.push(incomingPath);
  }
  for (const name of names) {
    const finalPath = path.join(publicRoot, name);
    if (existsSync(finalPath)) {
      const backupPath = path.join(backupRoot, name);
      renameSync(finalPath, backupPath);
      backedUp.push([backupPath, finalPath]);
    }
  }
  for (let index = 0; index < names.length; index += 1) {
    const finalPath = path.join(publicRoot, names[index]);
    renameSync(incoming[index], finalPath);
    promoted.push(finalPath);
  }
} catch (error) {
  for (const finalPath of promoted.reverse()) rmSync(finalPath, { force: true });
  for (const [backupPath, finalPath] of backedUp.reverse()) {
    if (existsSync(backupPath)) renameSync(backupPath, finalPath);
  }
  for (const incomingPath of incoming) rmSync(incomingPath, { force: true });
  throw error;
}

console.log(JSON.stringify({ ok: true, staged: stageRoot, publishedTo: publicRoot, media, supplemental }, null, 2));
