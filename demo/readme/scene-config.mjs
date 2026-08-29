import { DEMO_SPEC } from './storyboard.mjs';

const CURSOR_HOME = Object.freeze({ x: 1371, y: 850 });

async function resetScene(page) {
  await page.evaluate(() => window.__eclipseDemo.reset());
}

async function assertSnapshot(page, expected, label) {
  const snapshot = await page.evaluate(() => window.__eclipseDemo.snapshot());
  for (const [key, value] of Object.entries(expected)) {
    if (snapshot[key] !== value) {
      throw new Error(`${label}: expected ${key}=${JSON.stringify(value)}, received ${JSON.stringify(snapshot[key])}`);
    }
  }
}

export async function createScene({ url, out, reviewDir, logLevel = 'info' }) {
  const [{ render, timeline, web, dryRun, contactSheet, estimateSeconds }, { cursor }] = await Promise.all([
    import('gifsmith'),
    import('gifsmith/props'),
  ]);

  const tl = timeline((t) => {
    t.waitFor('#demo-root');
    t.waitUntil(() => document.documentElement.dataset.ready === 'true');
    t.call(resetScene, { name: 'reset-original-scene', seconds: 0 });
    t.call((page) => assertSnapshot(page, { state: 'home', activePanel: 'home', talking: false }, 'opening state'), { name: 'assert-opening-state', seconds: 0 });

    t.loopAnchor();
    t.hold(DEMO_SPEC.openingAnchorHoldSeconds);
    t.cue('console-ready');

    t.click('#nav-games', { via: 'cursor', glideSeconds: 0.65 });
    t.hold(2.8);
    t.click('#game-eclipse', { via: 'cursor', glideSeconds: 0.65 });
    t.hold(3.0);
    t.cue('game-selected');
    t.call((page) => assertSnapshot(page, { state: 'profile', activePanel: 'profile' }, 'selected profile'), { name: 'assert-profile-selected', seconds: 0 });

    t.click('#scan-game', { via: 'cursor', glideSeconds: 0.65 });
    t.hold(2.8);
    t.cue('scan-complete');
    t.call((page) => assertSnapshot(page, { state: 'ready', activePanel: 'ready' }, 'completed scan'), { name: 'assert-scan-complete', seconds: 0 });
    t.hold(1.4);

    t.click('#start-simulation', { via: 'cursor', glideSeconds: 0.65 });
    t.hold(2.2);
    t.cue('simulation-started');
    t.call((page) => assertSnapshot(page, { state: 'game', activePanel: 'game', talking: false }, 'started simulation'), { name: 'assert-simulation-started', seconds: 0 });
    t.hold(1.3);

    t.click('#ptt-button', { via: 'cursor', glideSeconds: 0.65 });
    t.cue('player-speaking');
    t.hold(5.0);
    t.cue('mara-responding');
    t.call((page) => assertSnapshot(page, { state: 'speaking', activePanel: 'game', activeStage: 'animating', talking: true }, 'Mara response'), { name: 'assert-anchored-response', seconds: 0 });
    t.hold(6.5);

    t.click('#stop-session', { via: 'cursor', glideSeconds: 0.65 });
    t.cue('session-stopped');
    t.hold(1.5);
    t.call((page) => assertSnapshot(page, { state: 'stopped', activePanel: 'game', talking: false }, 'stopped session'), { name: 'assert-session-stopped', seconds: 0 });

    t.click('#close-simulation', { via: 'cursor', glideSeconds: 0.65 });
    t.cursorTo(CURSOR_HOME, 1.0, 'easeInOut');
    t.cue('console-restored');
    t.call((page) => assertSnapshot(page, { state: 'home', activePanel: 'home', talking: false }, 'restored console'), { name: 'assert-console-restored', seconds: 0 });
    t.hold(1.8);
  });

  const target = web(url);
  const config = {
    target,
    out,
    format: 'gif',
    alsoEmit: ['webp', 'mp4'],
    mp4Sidecar: true,
    viewport: DEMO_SPEC.viewport,
    compose: 'overlay',
    props: [cursor({ start: CURSOR_HOME })],
    timeline: tl,
    loop: { strategy: 'anchor', minCycleSeconds: DEMO_SPEC.minimumCycleSeconds },
    capture: { mode: 'deterministic', format: 'png', frameTimeoutMs: 20_000 },
    encode: {
      width: DEMO_SPEC.output.width,
      fps: DEMO_SPEC.output.fps,
      speed: 1.15,
      colors: DEMO_SPEC.output.colors,
      dither: 'none',
      palette: 'full',
      quality: 91,
      mp4Crf: 16,
      targetMB: 28,
    },
    review: reviewDir ? { dir: reviewDir, maxFindings: 10, controls: 4 } : false,
    logLevel,
  };

  return {
    config,
    helpers: { render, dryRun, contactSheet, estimateSeconds },
    plannedSeconds: estimateSeconds(tl.steps),
  };
}
