const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright-core');
const output = process.env.NPC2_UI_REVIEW_OUTPUT || 'E:/temp/InteractiveNPCs/cyberpunk-ui-20260908';
const base = process.env.NPC2_UI_URL || 'http://127.0.0.1:1426';

async function main() {
  fs.mkdirSync(output, {recursive: true});
  const browser = await chromium.launch({headless: true, channel: 'msedge'});
  const results = [];
  try {
    for (const [name, pageId, width, height] of [
      ['channel-wide', 'session', 1440, 1000],
      ['world-wide', 'world', 1440, 1000],
      ['loadout-wide', 'voice', 1600, 1000],
      ['settings-wide', 'settings', 1440, 1000],
      ['loadout-narrow', 'voice', 720, 1000],
      ['channel-narrow', 'session', 720, 1000],
      ['setup-wide', 'session&onboarding=1', 1440, 1000],
      ['setup-narrow', 'session&onboarding=1', 720, 1000],
    ]) {
      const page = await browser.newPage({viewport: {width, height}, reducedMotion: 'reduce'});
      const errors = [];
      page.on('pageerror', e => errors.push(e.message));
      await page.goto(`${base}/?page=${pageId}`, {waitUntil: 'networkidle'});
      try { await page.locator('.product-shell').waitFor(); }
      catch (e) { console.log(errors); console.log(await page.locator('body').innerText()); await page.screenshot({path:path.join(output, 'boot-error.png')}); throw e; }
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({path:path.join(output,`${name}.png`), animations:'disabled'});
      const metrics = await page.evaluate(() => ({
        bodyOverflow: document.documentElement.scrollWidth > innerWidth,
        mainOverflow: document.querySelector('.product-main').scrollWidth > document.querySelector('.product-main').clientWidth,
        title: document.querySelector('h1')?.textContent,
        primaryNavigation: [...document.querySelectorAll('.product-rail nav button')].map(x=>x.textContent),
      }));
      results.push({name, ...metrics, errors});
      if(name.endsWith('-wide')) {
        const regions = name === 'loadout-wide'
          ? ['.product-rail','.page-heading','.neural-map','.loadout-role']
          : name === 'channel-wide'
            ? ['.product-rail','.panel-heading','.turn-composer','.session-side']
            : name === 'world-wide'
              ? ['.product-rail','.page-heading','.game-target-compact','.practice-lab']
              : ['.product-rail','.page-heading','.workspace-section-nav','.product-preferences-workspace'];
        for (let i=0;i<regions.length;i++) {
          const region=page.locator(regions[i]).first();
          if(await region.isVisible()) await region.screenshot({path:path.join(output,`${name}-detail-${i+1}.png`),animations:'disabled'});
        }
        await page.locator('.product-main').evaluate(el=>el.scrollTop=0);
      }
      if (name === 'loadout-wide') {
        for (const [role, label] of [['tts','Character voice.'],['stt','Speech recognition.'],['lipSync','Optional lip-sync.']]) {
          await page.getByRole('button', {name: new RegExp('^' + label.replace('.', '\\.'))}).click();
          await page.screenshot({path:path.join(output,`loadout-${role}.png`),animations:'disabled'});
        }
        await page.getByRole('button', {name: /^Accounts/}).click();
        await page.getByLabel('Provider account', {exact:true}).selectOption('elevenlabs');
        await page.screenshot({path:path.join(output,'accounts-elevenlabs.png'),animations:'disabled'});
        await page.getByRole('button', {name: /^Microphone/}).click();
        await page.screenshot({path:path.join(output,'microphone-wide.png'),animations:'disabled'});
      }
      await page.close();
    }
    fs.writeFileSync(path.join(output,'report.json'),JSON.stringify(results,null,2));
    console.log(JSON.stringify(results,null,2));
    if(results.some(r=>r.bodyOverflow || r.mainOverflow || r.errors.length)) process.exitCode=1;
  } finally {await browser.close();}
}
main().catch(e=>{console.error(e);process.exitCode=1;});
