// Isolated component layout check. Uses the existing unit-test data contracts,
// never presents these mocked responses as native or live application proof.
const fs = require('node:fs');
const path = require('node:path');
const ts = require('../apps/control/node_modules/typescript');
const {chromium} = require('playwright-core');
const output = 'E:/temp/InteractiveNPCs/cyberpunk-preferences-layout-20260908';
const source = fs.readFileSync(path.join(__dirname,'../apps/control/src/ProductPreferencesWorkspace.test.tsx'),'utf8');
const fixtureSource = source.slice(source.indexOf('const effectiveValue'),source.indexOf('describe("native product preferences"'));
const js = ts.transpileModule(fixtureSource,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.React}}).outputText;
const fixtures = new Function(`${js}; return {preferences:preferenceSnapshot(), subtitles:subtitleSnapshot(), configuration:configurationSnapshot()};`)();

async function main() {
  fs.mkdirSync(output,{recursive:true});
  const browser = await chromium.launch({headless:true,channel:'msedge'});
  const reports=[];
  try {
    for (const width of [1440,720]) {
      const page=await browser.newPage({viewport:{width,height:1000},reducedMotion:'reduce'});
      const errors=[];
      page.on('pageerror',e=>errors.push(e.message));
      await page.addInitScript(data=>{
        window.__TAURI_INTERNALS__={invoke:async (command)=>{
          if(command==='product_preferences_snapshot') return data.preferences;
          if(command==='read_subtitle_preferences') return data.subtitles;
          if(command==='effective_configuration_snapshot') return data.configuration;
          throw new Error('Layout fixture has no mutation handler: '+command);
        }};
      },fixtures);
      await page.route(/\/src\/main\.tsx(?:\?.*)?$/,route=>route.fulfill({contentType:'text/javascript',body:`
        import React from '/node_modules/.vite/deps/react.js';
        import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
        import {ProductPreferencesWorkspace} from '/src/ProductPreferencesWorkspace.tsx';
        import '/src/styles.css'; import '/src/product.css'; import '/src/review.css'; import '/src/cyberpunk.css';
        const h=React.createElement;
        ReactDOM.createRoot(document.getElementById('root')).render(h('div',{className:'product-shell cyberpunk-shell'},
          h('main',{className:'product-main',style:{height:'100vh',paddingTop:24}},
            h('header',{className:'page-heading'},h('span',{className:'eyebrow'},'LAYOUT FIXTURE / NO NATIVE ACTIONS'),h('h1',null,'Conversation settings')),
            h(ProductPreferencesWorkspace,{nativeAvailable:true,gameProfileId:'cyberpunk-2077',characterId:'misty-olszewski'}))));
      `}));
      await page.goto('http://127.0.0.1:1426',{waitUntil:'networkidle'});
      try { await page.getByRole('heading',{name:'Conversation settings'}).waitFor(); }
      catch(e) { console.log(errors); console.log(await page.locator('body').innerText()); throw e; }
      await page.locator('.preference-preset-grid--primary').waitFor();
      await page.getByText('Loading native preferences…').waitFor({state:'hidden'});
      await page.screenshot({path:path.join(output,`settings-${width}.png`),animations:'disabled'});
      for(const [name,selector] of [['presets','.preference-preset-grid--primary'],['controls','.preference-overrides'],['subtitles','.subtitle-preferences-workspace']]) {
        const locator=page.locator(selector).first();
        if(await locator.count()) {await locator.screenshot({path:path.join(output,`${name}-${width}.png`),animations:'disabled'});}
      }
      reports.push({width,errors,fixtureOnly:true,overflow:await page.locator('.product-main').evaluate(el=>el.scrollWidth>el.clientWidth)});
      await page.close();
    }
  } finally {await browser.close();}
  fs.writeFileSync(path.join(output,'report.json'),JSON.stringify(reports,null,2));
  console.log(JSON.stringify(reports));
  if(reports.some(r=>r.errors.length || r.overflow)) process.exitCode=1;
}
main().catch(e=>{console.error(e);process.exitCode=1;});
