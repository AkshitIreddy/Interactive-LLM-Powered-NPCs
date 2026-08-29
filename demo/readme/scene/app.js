const root = document.documentElement;
const gameWorld = document.querySelector('.game-world');
const stopCard = document.querySelector('#stop-card');
const playerCaption = document.querySelector('.player-caption');
const npcCaption = document.querySelector('.npc-caption');
const playerLine = document.querySelector('#player-line');
const maraLine = document.querySelector('#mara-line');
const speakerStatus = document.querySelector('#speaker-status');
const spineTotal = document.querySelector('#spine-total');
const spineItems = [...document.querySelectorAll('#response-spine li')];
const navItems = [...document.querySelectorAll('.nav-item[data-view]')];
const timers = new Set();

const stages = [
  ['listening', 'LIVE', 'Listening…', '—'],
  ['transcribing', '92 ms', 'Local speech', '92ms'],
  ['identifying', '48 ms', 'Mara Venn · 0.96', '48ms'],
  ['remembering', '61 ms', '3 relevant memories', '61ms'],
  ['responding', 'LIVE', 'Streaming first clause', '1.0s'],
  ['voicing', '112 ms', 'Local synthesis', '112ms'],
  ['animating', '34 ms', 'Anchored visemes', '34ms'],
];

function after(ms, fn) {
  const id = window.setTimeout(() => {
    timers.delete(id);
    fn();
  }, ms);
  timers.add(id);
  return id;
}

function clearTimers() {
  for (const id of timers) window.clearTimeout(id);
  timers.clear();
}

function setPanel(panel) {
  for (const view of document.querySelectorAll('.view')) {
    const active = view.dataset.panel === panel;
    view.classList.toggle('active', active);
    view.setAttribute('aria-hidden', String(!active));
  }
}

function setNavigation(name) {
  for (const item of navItems) item.classList.toggle('selected', item.dataset.view === name);
}

function resetSpine() {
  for (const item of spineItems) {
    item.classList.remove('active', 'complete');
    item.querySelector('em').textContent = '—';
  }
  spineTotal.textContent = 'READY';
}

function activateSpine(index) {
  spineItems.forEach((item, i) => {
    item.classList.toggle('complete', i < index);
    item.classList.toggle('active', i === index);
    if (i < index) item.querySelector('em').textContent = stages[i][3];
    if (i === index) item.querySelector('em').textContent = stages[i][1];
  });
  spineTotal.textContent = `${index + 1} / 7`;
}

function setScanProgress(step) {
  const order = ['scan-step-target', 'scan-step-safety', 'scan-step-media'];
  order.forEach((id, i) => {
    const el = document.getElementById(id);
    el.classList.toggle('done', i < step);
    el.classList.toggle('active', i === step);
    el.querySelector('span').textContent = i < step ? '✓' : '';
    el.querySelector('em').textContent = i < step ? 'Verified' : i === step ? 'Scanning' : 'Queued';
  });
  const copy = [
    ['Locating Eclipse Harbor…', 'Checking the isolated simulation fixture. No real game process is inspected in this demo.'],
    ['Verifying safety boundary…', 'The fixture is single-player, offline, and contains no anti-cheat component.'],
    ['Rehearsing media path…', 'Testing deterministic audio, subtitle, and anchored viseme events.'],
    ['All checks complete', 'The original Eclipse Harbor simulation is ready to start.'],
  ][step];
  document.getElementById('scan-title').textContent = copy[0];
  document.getElementById('scan-detail').textContent = copy[1];
}

function setState(state) {
  root.dataset.demoState = state;
  stopCard.classList.remove('visible');
  gameWorld.classList.remove('talking');

  if (['game', 'listening', 'thinking', 'speaking', 'stopped'].includes(state)) {
    setPanel('game');
  } else if (state === 'profile') {
    setPanel('profile');
  } else if (state === 'scanning') {
    setPanel('scanning');
  } else if (state === 'ready') {
    setPanel('ready');
  } else {
    setPanel(state);
  }

  if (state === 'home') setNavigation('home');
  if (['games', 'profile', 'scanning', 'ready'].includes(state)) setNavigation('games');
  if (state === 'listening') {
    activateSpine(0);
    playerLine.textContent = 'Mara, did the north beacon answer?';
    playerCaption.classList.add('visible');
    npcCaption.classList.remove('visible');
    speakerStatus.textContent = 'Listening · selected';
  }
  if (state === 'thinking') speakerStatus.textContent = 'Mara Venn · identified';
  if (state === 'speaking') {
    gameWorld.classList.add('talking');
    npcCaption.classList.add('visible');
    maraLine.textContent = 'It answered once — three short pulses from beyond the fog. No ship code I know.';
    speakerStatus.textContent = 'Speaking · anchored visemes';
  }
  if (state === 'stopped') {
    stopCard.classList.add('visible');
    speakerStatus.textContent = 'Session stopped';
  }
}

function reset() {
  clearTimers();
  resetSpine();
  setScanProgress(0);
  playerCaption.classList.remove('visible');
  npcCaption.classList.remove('visible');
  playerLine.textContent = 'Hold Left Alt to speak';
  maraLine.textContent = 'The north beacon is quiet. For now.';
  speakerStatus.textContent = 'Nearby · selected';
  setState('home');
}

function runScan() {
  clearTimers();
  setScanProgress(0);
  setState('scanning');
  after(620, () => setScanProgress(1));
  after(1230, () => setScanProgress(2));
  after(1810, () => setScanProgress(3));
  after(2260, () => setState('ready'));
}

function startSimulation() {
  clearTimers();
  resetSpine();
  playerCaption.classList.remove('visible');
  npcCaption.classList.remove('visible');
  playerLine.textContent = 'Hold Left Alt to speak';
  maraLine.textContent = 'The north beacon is quiet. For now.';
  speakerStatus.textContent = 'Nearby · selected';
  setState('game');
}

function runConversation() {
  clearTimers();
  resetSpine();
  setState('listening');
  after(860, () => { setState('thinking'); activateSpine(1); });
  after(1560, () => activateSpine(2));
  after(2220, () => activateSpine(3));
  after(2910, () => activateSpine(4));
  after(4010, () => activateSpine(5));
  after(4690, () => { activateSpine(6); setState('speaking'); });
}

document.getElementById('nav-games').addEventListener('click', () => setState('games'));
document.getElementById('home-open-games').addEventListener('click', () => setState('games'));
document.getElementById('nav-home').addEventListener('click', () => setState('home'));
document.getElementById('game-eclipse').addEventListener('click', () => setState('profile'));
document.getElementById('scan-game').addEventListener('click', runScan);
document.getElementById('start-simulation').addEventListener('click', startSimulation);
document.getElementById('ptt-button').addEventListener('click', runConversation);
document.getElementById('stop-session').addEventListener('click', () => { clearTimers(); setState('stopped'); });
document.getElementById('close-simulation').addEventListener('click', reset);

for (const item of navItems) {
  if (['home', 'games'].includes(item.dataset.view)) continue;
  item.addEventListener('click', () => {
    // The production demo focuses on Home and Games; other destinations remain
    // visibly present to communicate the complete product information model.
  });
}

const wave = document.getElementById('voice-wave');
for (let i = 0; i < 38; i += 1) {
  const bar = document.createElement('i');
  bar.style.setProperty('--n', String(i));
  wave.append(bar);
}

window.__eclipseDemo = Object.freeze({
  reset,
  setState,
  setScanProgress,
  activateSpine,
  runScan,
  startSimulation,
  runConversation,
  snapshot: () => ({
    state: root.dataset.demoState,
    activePanel: document.querySelector('.view.active')?.dataset.panel ?? null,
    activeStage: document.querySelector('.response-spine li.active')?.dataset.stage ?? null,
    talking: gameWorld.classList.contains('talking'),
  }),
});

reset();
root.dataset.ready = 'true';
