import { createServer } from 'node:http';
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import ffmpegPath from 'ffmpeg-static';
import ffprobeStatic from 'ffprobe-static';
import { createScene } from './scene-config.mjs';
import { DEMO_SPEC } from './storyboard.mjs';
import { sha256, verifyMediaSet } from './verify-lib.mjs';

process.env.FFMPEG_PATH = ffmpegPath;

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '..', '..');
const sceneRoot = path.join(here, 'scene');
const publicRoot = path.join(repoRoot, 'docs', 'assets', 'demo');
const workRoot = path.join(here, '.render-work');
const runId = new Date().toISOString().replace(/[:.]/g, '-');
const stageRoot = path.join(workRoot, runId);
const reviewDir = path.join(stageRoot, 'temporal-review');
const args = new Set(process.argv.slice(2));

const mime = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.css', 'text/css; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.svg', 'image/svg+xml'],
  ['.png', 'image/png'],
]);

function startServer() {
  const server = createServer((request, response) => {
    const requestPath = decodeURIComponent(new URL(request.url, 'http://127.0.0.1').pathname);
    const relative = requestPath === '/' ? 'index.html' : requestPath.replace(/^\/+/, '');
    const file = path.resolve(sceneRoot, relative);
    if (!file.startsWith(`${sceneRoot}${path.sep}`) || !existsSync(file)) {
      response.writeHead(404).end('Not found');
      return;
    }
    response.setHeader('Cache-Control', 'no-store');
    response.setHeader('Content-Type', mime.get(path.extname(file)) ?? 'application/octet-stream');
    response.end(readFileSync(file));
  });
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      resolve({ server, url: `http://127.0.0.1:${address.port}/` });
    });
  });
}

function buildDerivedMedia(mp4) {
  execFileSync(ffmpegPath, ['-y', '-ss', '18', '-i', mp4, '-frames:v', '1', path.join(stageRoot, 'poster.png')], { stdio: 'inherit' });
  execFileSync(ffmpegPath, ['-y', '-i', mp4, '-vf', 'fps=1/5,scale=480:-1,tile=3x2:padding=6:margin=6:color=0x071015', '-frames:v', '1', path.join(stageRoot, 'contact-sheet.png')], { stdio: 'inherit' });
}

function promoteTransaction(names) {
  mkdirSync(publicRoot, { recursive: true });
  const backupRoot = path.join(stageRoot, 'promotion-backup');
  mkdirSync(backupRoot, { recursive: true });
  const incoming = [];
  const backedUp = [];
  const promoted = [];
  try {
    for (const name of names) {
      const incomingPath = path.join(publicRoot, `.${name}.${runId}.incoming`);
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
    for (let i = 0; i < names.length; i += 1) {
      const finalPath = path.join(publicRoot, names[i]);
      renameSync(incoming[i], finalPath);
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
}

mkdirSync(stageRoot, { recursive: true });
const { server, url } = await startServer();
try {
  const { config, helpers, plannedSeconds } = await createScene({
    url,
    out: path.join(stageRoot, 'demo.gif'),
    reviewDir,
    logLevel: 'info',
  });

  if (plannedSeconds < DEMO_SPEC.expectedMinimumSeconds || plannedSeconds > DEMO_SPEC.expectedMaximumSeconds) {
    throw new Error(`Storyboard duration ${plannedSeconds.toFixed(2)}s is outside ${DEMO_SPEC.expectedMinimumSeconds}-${DEMO_SPEC.expectedMaximumSeconds}s`);
  }

  const dry = await helpers.dryRun(config);
  writeFileSync(path.join(stageRoot, 'dry-run.json'), `${JSON.stringify(dry, null, 2)}\n`);
  if (!dry.ok) throw new Error(`Gifsmith dry run failed:\n${dry.errors.join('\n')}`);

  if (args.has('--dry-run')) {
    console.log(JSON.stringify({ stageRoot, plannedSeconds, ...dry }, null, 2));
    process.exitCode = 0;
  } else if (args.has('--contact-sheet')) {
    const sheet = await helpers.contactSheet({ ...config, review: false }, 6);
    const target = path.join(stageRoot, 'contact-sheet.png');
    writeFileSync(target, Buffer.from(sheet.gridBase64, 'base64'));
    sheet.frames.forEach((frame, index) => {
      writeFileSync(path.join(stageRoot, `contact-${String(index + 1).padStart(2, '0')}.png`), Buffer.from(frame.base64, 'base64'));
    });
    console.log(JSON.stringify({ target, times: sheet.times, columns: sheet.columns }, null, 2));
  } else {
    const startedAt = new Date().toISOString();
    const result = await helpers.render(config);
    writeFileSync(path.join(stageRoot, 'render-result.json'), `${JSON.stringify(result, null, 2)}\n`);
    buildDerivedMedia(path.join(stageRoot, 'demo.mp4'));
    const media = verifyMediaSet(stageRoot, ffprobeStatic.path);

    const supplemental = {};
    for (const name of ['poster.png', 'contact-sheet.png']) {
      supplemental[name] = { bytes: readFileSync(path.join(stageRoot, name)).byteLength, sha256: sha256(path.join(stageRoot, name)) };
    }

    const manifest = {
      schemaVersion: 1,
      production: 'Eclipse Harbor — Response Console 2.0',
      copyrightSafe: true,
      deterministic: true,
      illustrativePerformanceHud: true,
      startedAt,
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
      plan: { plannedSeconds, cues: DEMO_SPEC.cues },
      result,
      media,
      supplemental,
      notes: [
        'All art, names, dialogue, and interface material are original to this repository.',
        'The HUD values are illustrative and are not benchmark measurements.',
        'Render throughput is intentionally omitted because deterministic capture decouples it from playback quality.',
      ],
    };
    writeFileSync(path.join(stageRoot, 'render-manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
    promoteTransaction(['demo.gif', 'demo.webp', 'demo.mp4', 'poster.png', 'contact-sheet.png', 'render-manifest.json']);
    console.log(JSON.stringify({ publishedTo: publicRoot, stageRoot, plannedSeconds, result, media }, null, 2));
  }
} catch (error) {
  console.error(`README demo failed. Existing approved assets were not replaced. Staging retained at ${stageRoot}`);
  throw error;
} finally {
  await new Promise((resolve) => server.close(resolve));
}
