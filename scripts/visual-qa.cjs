#!/usr/bin/env node

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
let playwright;
try {
  playwright = require('playwright-core');
} catch (error) {
  try {
    playwright = require('playwright');
  } catch {
    const bundled = process.env.USERPROFILE
      ? path.join(process.env.USERPROFILE, '.cache', 'codex-runtimes', 'codex-primary-runtime', 'dependencies', 'node', 'node_modules', 'playwright')
      : null;
    if (!bundled || !fs.existsSync(bundled)) throw error;
    playwright = require(bundled);
  }
}
const { chromium } = playwright;

const baseUrl = process.env.NPC2_UI_URL || 'http://127.0.0.1:1420';
const outputRoot = path.resolve(process.env.NPC2_VISUAL_OUTPUT || 'artifacts/visual-qa');

const wide = { width: 1440, height: 900 };
const narrow = { width: 720, height: 900 };
const surfaces = [
  { id: 'theme-specimen', url: '/?themeSpecimen=1', viewport: { width: 1500, height: 960 }, ready: '.theme-specimen', closeups: ['.specimen-card--settings', '.specimen-card--cyberware', '.specimen-card--scanner'] },
  { id: 'home-ready', url: '/?page=home&state=ready', viewport: wide, closeups: ['.sidebar', '.response-spine', '.command-deck', '.home-grid'] },
  { id: 'home-active', url: '/?page=home&state=active', viewport: wide, closeups: ['.sidebar', '.response-spine', '.command-deck', '.home-grid'] },
  { id: 'home-loading', url: '/?page=home&state=loading', viewport: wide, closeups: ['.response-spine', '.state-banner', '.command-deck'] },
  { id: 'home-empty', url: '/?page=home&state=empty', viewport: wide, closeups: ['.response-spine', '.state-banner', '.command-deck'] },
  { id: 'home-degraded', url: '/?page=home&state=degraded', viewport: wide, closeups: ['.response-spine', '.state-banner', '.command-deck'] },
  { id: 'games-authored', url: '/?page=games&state=ready', viewport: wide, closeups: ['.page-hero', '.library-toolbar'], expectedCounts: [{ selector: '.profile-card', count: 21 }], scrollCaptures: [0, 650, 1300, 1950] },
  { id: 'characters-ready', url: '/?page=characters&state=ready', viewport: wide, closeups: ['.page-hero', '.character-workspace'] },
  { id: 'conversation-ready', url: '/?page=conversation&state=ready', viewport: wide, closeups: ['.page-hero', '.conversation-layout'] },
  { id: 'presence-ready', url: '/?page=presence&state=ready', viewport: wide, closeups: ['.page-hero', '.presence-hero', '.settings-sections'] },
  { id: 'performance-ready', url: '/?page=performance&state=ready', viewport: wide, closeups: ['.page-hero', '.performance-summary', '.performance-grid'] },
  { id: 'models-ready', url: '/?page=models&state=ready', viewport: wide, closeups: ['.page-hero', '.model-summary', '.model-table'] },
  { id: 'models-loading', url: '/?page=models&state=loading', viewport: wide, closeups: ['.sidebar', '.response-spine', '.page-hero', '.loading-grid'] },
  { id: 'models-error', url: '/?page=models&state=error', viewport: wide, closeups: ['.state-banner', '.model-summary', '.model-table'] },
  { id: 'diagnostics-error', url: '/?page=diagnostics&state=error', viewport: wide, closeups: ['.page-hero', '.incident-card', '.diagnostics-grid'] },
  { id: 'settings-general', url: '/?page=settings&settingsSection=general', viewport: wide, closeups: ['.page-hero', '.settings-nav', '.settings-main', '.settings-context'] },
  { id: 'settings-privacy', url: '/?page=settings&settingsSection=privacy', viewport: wide, closeups: ['.page-hero', '.settings-nav', '.settings-main', '.settings-context'] },
  { id: 'settings-providers', url: '/?page=settings&settingsSection=providers', viewport: wide, closeups: ['.page-hero', '.loadout-console', '.provider-directory', '.provider-detail', '.experiment-paths'], scrollCaptures: [0, 780, 1560, 2340] },
  { id: 'settings-nvidia-nim', url: '/?page=settings&settingsSection=providers&provider=nvidia-nim', viewport: wide, closeups: ['.provider-directory', '.provider-detail', '.provider-guidance', '.experiment-paths'] },
  { id: 'settings-accessibility', url: '/?page=settings&settingsSection=accessibility', viewport: wide, closeups: ['.page-hero', '.settings-nav', '.settings-main', '.settings-context'] },
  { id: 'help-ready', url: '/?page=help&state=ready', viewport: wide, closeups: ['.help-hero', '.help-grid'] },
  { id: 'onboarding-welcome', url: '/?onboarding=1&step=0', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'onboarding-this-pc', url: '/?onboarding=1&step=1', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'onboarding-run-mode', url: '/?onboarding=1&step=2', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'onboarding-game', url: '/?onboarding=1&step=3', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'onboarding-providers', url: '/?onboarding=1&step=4', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'onboarding-rehearsal', url: '/?onboarding=1&step=5', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'onboarding-presence', url: '/?onboarding=1&step=6', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'onboarding-performance', url: '/?onboarding=1&step=7', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'onboarding-test-run', url: '/?onboarding=1&step=8', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'onboarding-ready', url: '/?onboarding=1&step=9', viewport: wide, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
  { id: 'settings-light', url: '/?page=settings&settingsSection=general&theme=light', viewport: wide, closeups: ['.topbar', '.page-hero', '.settings-workspace'] },
  { id: 'settings-high-contrast', url: '/?page=settings&settingsSection=accessibility&contrast=high&motion=reduce', viewport: wide, closeups: ['.topbar', '.page-hero', '.settings-workspace'] },
  { id: 'settings-large-text', url: '/?page=settings&settingsSection=accessibility&largeText=1', viewport: wide, closeups: ['.topbar', '.page-hero', '.settings-workspace'] },
  { id: 'providers-narrow', url: '/?page=settings&settingsSection=providers', viewport: narrow, closeups: ['.response-spine', '.settings-nav', '.loadout-console', '.provider-layout'], scrollCaptures: [0, 700, 1400, 2100, 2800, 3500] },
  { id: 'onboarding-narrow', url: '/?onboarding=1&step=2', viewport: narrow, ready: '.onboarding', closeups: ['.onboarding__rail', '.onboarding__header', '.onboarding__content', '.onboarding__footer'] },
];

function commandOutput(file, args) {
  try {
    return execFileSync(file, args, { cwd: process.cwd(), encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    return null;
  }
}

function hashFile(relativePath) {
  const fullPath = path.resolve(relativePath);
  if (!fs.existsSync(fullPath) || !fs.statSync(fullPath).isFile()) return null;
  return crypto.createHash('sha256').update(fs.readFileSync(fullPath)).digest('hex');
}

function sourceIdentity() {
  const files = [];
  const visit = (root) => {
    if (!fs.existsSync(root)) return;
    for (const entry of fs.readdirSync(root, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      if (['dist', 'node_modules', 'target'].includes(entry.name)) continue;
      const candidate = path.join(root, entry.name);
      if (entry.isDirectory()) visit(candidate);
      else if (entry.isFile()) files.push(path.relative(process.cwd(), candidate).replaceAll('\\', '/'));
    }
  };
  visit(path.resolve('apps/control/src'));
  visit(path.resolve('apps/control/public'));
  files.push('apps/control/index.html', 'apps/control/package.json');
  files.push('scripts/visual-qa.cjs');
  files.sort();
  const digest = crypto.createHash('sha256');
  for (const file of files) {
    digest.update(file).update('\0').update(fs.readFileSync(path.resolve(file))).update('\0');
  }
  return {
    head: commandOutput('git', ['rev-parse', 'HEAD']),
    branch: commandOutput('git', ['branch', '--show-current']),
    dirty: Boolean(commandOutput('git', ['status', '--porcelain'])),
    evidenceClass: 'mutable-local-review',
    immutableRcEvidence: false,
    scopedSourceFiles: files.length,
    scopedSourceSha256: digest.digest('hex'),
    lockfiles: {
      pnpm: hashFile('pnpm-lock.yaml'),
      tauriCargo: hashFile('apps/control/src-tauri/Cargo.lock'),
    },
  };
}

async function main() {
  fs.mkdirSync(outputRoot, { recursive: true });
  const launchOptions = { headless: true };
  if (process.env.NPC2_CHROMIUM_EXECUTABLE) {
    launchOptions.executablePath = process.env.NPC2_CHROMIUM_EXECUTABLE;
  } else if (process.platform === 'win32') {
    launchOptions.channel = 'msedge';
  }
  const browser = await chromium.launch(launchOptions);
  const results = [];

  try {
    for (const surface of surfaces) {
      const context = await browser.newContext({ viewport: surface.viewport, deviceScaleFactor: 1 });
      const page = await context.newPage();
      await page.emulateMedia({ reducedMotion: 'reduce' });
      const consoleErrors = [];
      const pageErrors = [];
      page.on('console', (message) => {
        if (message.type() === 'error') consoleErrors.push(message.text());
      });
      page.on('pageerror', (error) => pageErrors.push(error.message));

      await page.goto(`${baseUrl}${surface.url}`, { waitUntil: 'networkidle' });
      await page.locator(surface.ready || '#main-content').waitFor({ state: 'visible' });
      await page.evaluate(async () => {
        document.documentElement.classList.add('force-reduced-motion');
        await document.fonts.ready;
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      });

      const overflow = await page.evaluate(() => ({
        documentWidth: document.documentElement.scrollWidth,
        viewportWidth: document.documentElement.clientWidth,
        bodyWidth: document.body.scrollWidth,
      }));
      const fullPath = path.join(outputRoot, `${surface.id}.png`);
      await page.screenshot({ path: fullPath, fullPage: false });

      const closeups = [];
      for (let index = 0; index < surface.closeups.length; index += 1) {
        const selector = surface.closeups[index];
        const locator = page.locator(selector).first();
        if (await locator.count() === 0 || !(await locator.isVisible())) continue;
        const closeupPath = path.join(outputRoot, `${surface.id}-closeup-${index + 1}.png`);
        await locator.screenshot({ path: closeupPath });
        closeups.push({ selector, path: closeupPath });
      }

      const countAssertions = [];
      for (const assertion of surface.expectedCounts || []) {
        const actual = await page.locator(assertion.selector).count();
        countAssertions.push({ ...assertion, actual, passed: actual === assertion.count });
      }

      const scrollCaptures = [];
      for (let index = 0; index < (surface.scrollCaptures || []).length; index += 1) {
        const requestedTop = surface.scrollCaptures[index];
        const actualTop = await page.locator('#main-content').evaluate((element, top) => {
          element.scrollTo({ top, behavior: 'instant' });
          return element.scrollTop;
        }, requestedTop);
        await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
        const scrollPath = path.join(outputRoot, `${surface.id}-scroll-${index + 1}.png`);
        await page.screenshot({ path: scrollPath, fullPage: false });
        scrollCaptures.push({ requestedTop, actualTop, path: scrollPath });
      }

      results.push({
        id: surface.id,
        url: surface.url,
        viewport: surface.viewport,
        overflow,
        hasHorizontalOverflow: overflow.documentWidth > overflow.viewportWidth,
        consoleErrors,
        pageErrors,
        fullPath,
        closeups,
        countAssertions,
        scrollCaptures,
      });
      await context.close();
    }
  } finally {
    await browser.close();
  }

  const reportPath = path.join(outputRoot, 'report.json');
  const report = {
    schemaVersion: 2,
    generatedAtUtc: new Date().toISOString(),
    sourceIdentity: sourceIdentity(),
    automatedChecks: {
      surfaceCount: results.length,
      noConsoleOrPageErrors: results.every((result) => !result.consoleErrors.length && !result.pageErrors.length),
      noHorizontalOverflow: results.every((result) => !result.hasHorizontalOverflow),
    },
    humanReview: {
      status: 'pending',
      note: 'Each retained close-up and then each full frame must be inspected before promotion.',
    },
    surfaces: results,
  };
  fs.writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  const failures = results.filter((result) => result.hasHorizontalOverflow || result.consoleErrors.length || result.pageErrors.length || result.countAssertions.some((assertion) => !assertion.passed));
  process.stdout.write(`${JSON.stringify({ reportPath, surfaces: results.length, failures }, null, 2)}\n`);
  process.exitCode = failures.length ? 1 : 0;
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
