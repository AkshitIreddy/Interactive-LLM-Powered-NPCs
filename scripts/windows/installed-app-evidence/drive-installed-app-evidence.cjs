"use strict";

const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

function parseArgs(argv) {
  const result = {};
  for (let index = 2; index < argv.length; index += 2) {
    const key = argv[index];
    if (!key.startsWith("--") || index + 1 >= argv.length) throw new Error(`Invalid argument at ${index}: ${key}`);
    result[key.slice(2)] = argv[index + 1];
  }
  for (const required of ["endpoint", "output", "plan", "playwright-module", "run-id"]) {
    if (!result[required]) throw new Error(`Missing --${required}`);
  }
  return result;
}

function safeName(value) {
  return value.toLowerCase().replace(/[^a-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 96) || "unnamed";
}

function sha256(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, { encoding: "utf8", flag: "wx" });
}

function regexFromRule(pattern) {
  const insensitive = pattern.startsWith("(?i)");
  return new RegExp(insensitive ? pattern.slice(4) : pattern, insensitive ? "i" : "");
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function connectWithRetry(chromium, endpoint) {
  const deadline = Date.now() + 30_000;
  let lastError;
  while (Date.now() < deadline) {
    try { return await chromium.connectOverCDP(endpoint); }
    catch (error) { lastError = error; await new Promise((resolve) => setTimeout(resolve, 250)); }
  }
  throw lastError || new Error("Installed WebView CDP endpoint was not available within 30 seconds.");
}

async function pageProbe(page) {
  return page.evaluate(() => {
    const visible = (element) => {
      const rect = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
    };
    const controls = [...document.querySelectorAll("button,input,select,textarea,a[href],[role=button],[role=checkbox],[role=radio],[role=combobox]")]
      .filter(visible)
      .map((element, ordinal) => {
        const evidenceId = `installed-evidence-control-${ordinal}`;
        element.setAttribute("data-installed-evidence-id", evidenceId);
        const rect = element.getBoundingClientRect();
        const role = element.getAttribute("role") || ({ BUTTON: "button", SELECT: "combobox", TEXTAREA: "textbox", A: "link" }[element.tagName] || (element.tagName === "INPUT" ? ({ checkbox: "checkbox", radio: "radio" }[element.type] || "textbox") : element.tagName.toLowerCase()));
        const label = (element.getAttribute("aria-label") || element.getAttribute("title") || (element.labels?.[0]?.textContent) || element.textContent || element.getAttribute("placeholder") || "").trim().replace(/\s+/g, " ");
        const disabled = Boolean(element.disabled || element.getAttribute("aria-disabled") === "true");
        return { ordinal, evidence_id: evidenceId, tag: element.tagName.toLowerCase(), role, label, disabled, name: element.getAttribute("name"), type: element.getAttribute("type"), test_id: element.getAttribute("data-testid"), x: rect.x, y: rect.y, width: rect.width, height: rect.height };
      });
    const unlabeled = controls.filter((control) => !control.label && !control.disabled);
    return {
      title: document.title,
      href: location.href,
      viewport: { width: document.documentElement.clientWidth, height: document.documentElement.clientHeight },
      scroll: { width: document.documentElement.scrollWidth, height: document.documentElement.scrollHeight },
      horizontal_overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
      enabled_unlabeled_count: unlabeled.length,
      enabled_unlabeled_controls: unlabeled,
      controls
    };
  });
}

async function selectCloseupRects(page, minimum, maximum) {
  return page.evaluate(({ minimum, maximum }) => {
    const selectors = [
      "main [role=dialog]", "main .modal", "main .page-header", "main header", "main .session-signal-rail",
      "main .response-spine", "main .this-pc-benchmark", "main .runtime-evidence-banner", "main section", "main article",
      "main .panel", "main .card", "main fieldset"
    ];
    const candidates = [];
    const seen = new Set();
    // Playwright's full-page PNG excludes the browser's vertical scrollbar,
    // so CSS scrollWidth can be wider than the capturable bitmap. Close-up
    // clips must use clientWidth or a right-edge fallback tile is rejected.
    const pageWidth = document.documentElement.clientWidth;
    const pageHeight = Math.max(document.documentElement.scrollHeight, document.body?.scrollHeight || 0);
    for (const selector of selectors) {
      const elements = [...document.querySelectorAll(selector)];
      for (let ordinal = 0; ordinal < elements.length; ordinal += 1) {
        const element = elements[ordinal];
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        if (rect.width < 220 || rect.height < 80 || style.display === "none" || style.visibility === "hidden") continue;
        const x = Math.max(0, rect.left + scrollX);
        const y = Math.max(0, rect.top + scrollY);
        const width = Math.min(Math.max(0, pageWidth - x), rect.width);
        const height = Math.min(900, Math.max(0, pageHeight - y), rect.height);
        if (width < 1 || height < 1) continue;
        const key = `${Math.round(x / 8)}:${Math.round(y / 8)}:${Math.round(width / 8)}:${Math.round(height / 8)}`;
        if (seen.has(key)) continue;
        seen.add(key);
        candidates.push({ selector, ordinal, x, y, width, height, area: width * height });
      }
    }
    candidates.sort((a, b) => a.y - b.y || b.area - a.area);
    const picked = [];
    for (const candidate of candidates) {
      const overlaps = picked.some((item) => {
        const overlapX = Math.max(0, Math.min(item.x + item.width, candidate.x + candidate.width) - Math.max(item.x, candidate.x));
        const overlapY = Math.max(0, Math.min(item.y + item.height, candidate.y + candidate.height) - Math.max(item.y, candidate.y));
        return overlapX * overlapY > Math.min(item.area, candidate.area) * 0.75;
      });
      if (!overlaps) picked.push(candidate);
      if (picked.length === maximum) break;
    }
    while (picked.length < minimum) {
      const index = picked.length;
      const y = Math.min(Math.max(0, index * Math.max(240, Math.floor(pageHeight / minimum))), Math.max(0, pageHeight - 420));
      picked.push({ selector: "fallback-tile", x: 0, y, width: Math.min(pageWidth, 1000), height: Math.min(420, pageHeight - y), area: Math.min(pageWidth, 1000) * Math.min(420, pageHeight - y) });
    }
    return picked.slice(0, maximum);
  }, { minimum, maximum });
}

async function captureScreen(page, cdp, output, screenId, planCapture, metadata) {
  const folder = path.join(output, "screenshots", safeName(screenId));
  fs.mkdirSync(folder, { recursive: true });
  await page.evaluate(() => window.scrollTo({ top: 0, behavior: "instant" }));
  await page.waitForTimeout(250);
  const fullPath = path.join(folder, "full.png");
  await page.screenshot({ path: fullPath, fullPage: true, animations: "disabled" });
  const rects = await selectCloseupRects(page, planCapture.closeups_min, planCapture.closeups_max);
  const png = fs.readFileSync(fullPath);
  if (png.length < 24 || png.toString("ascii", 1, 4) !== "PNG") throw new Error(`Full-page capture is not a PNG: ${fullPath}`);
  const scale = Number(metadata?.profile?.device_scale_factor || 1);
  const bitmapWidth = png.readUInt32BE(16) / scale;
  const bitmapHeight = png.readUInt32BE(20) / scale;
  const boundedRects = [];
  for (const rect of rects) {
    const x = Math.max(0, Math.floor(rect.x));
    const y = Math.max(0, Math.floor(rect.y));
    if (x >= bitmapWidth || y >= bitmapHeight) continue;
    const width = Math.min(Math.max(1, Math.floor(rect.width)), Math.floor(bitmapWidth - x));
    const height = Math.min(Math.max(1, Math.floor(rect.height)), Math.floor(bitmapHeight - y));
    if (width < 1 || height < 1) continue;
    boundedRects.push({ ...rect, x, y, width, height });
  }
  while (boundedRects.length < planCapture.closeups_min) {
    const index = boundedRects.length;
    const width = Math.max(1, Math.min(1000, Math.floor(bitmapWidth)));
    const y = Math.min(
      Math.max(0, index * Math.max(200, Math.floor(bitmapHeight / planCapture.closeups_min))),
      Math.max(0, Math.floor(bitmapHeight) - 1),
    );
    const height = Math.max(1, Math.min(420, Math.floor(bitmapHeight - y)));
    boundedRects.push({ selector: "fallback-bitmap-tile", x: 0, y, width, height, area: width * height });
  }
  const closeups = [];
  for (let index = 0; index < boundedRects.length; index += 1) {
    const rect = boundedRects[index];
    const file = path.join(folder, `closeup-${String(index + 1).padStart(2, "0")}.png`);
    try {
      await page.screenshot({ path: file, clip: { x: rect.x, y: rect.y, width: Math.max(1, rect.width), height: Math.max(1, rect.height) }, animations: "disabled", captureBeyondViewport: true });
    } catch (error) {
      if (!Number.isInteger(rect.ordinal) || rect.selector.startsWith("fallback-")) {
        throw new Error(`Close-up capture failed for ${screenId}: ${JSON.stringify({ rect, bitmapWidth, bitmapHeight, scale })}: ${error.message}`);
      }
      const element = page.locator(rect.selector).nth(rect.ordinal);
      if (!(await element.isVisible())) {
        throw new Error(`Close-up fallback element is no longer visible for ${screenId}: ${JSON.stringify(rect)}`);
      }
      await element.scrollIntoViewIfNeeded();
      await element.screenshot({ path: file, animations: "disabled" });
      rect.capture_fallback = "indexed-element-screenshot";
    }
    closeups.push({ file: path.relative(output, file).replaceAll("\\", "/"), sha256: sha256(file), source_region: rect.selector, clip: rect });
  }
  const probe = await pageProbe(page);
  return { id: screenId, ...metadata, captured_at_utc: new Date().toISOString(), pixel_source: "webview-cdp", full: { file: path.relative(output, fullPath).replaceAll("\\", "/"), sha256: sha256(fullPath) }, closeups, probe };
}

async function navigateTo(page, pageSpec) {
  const accessibleName = new RegExp(`^(?:\\d{2}\\s+)?${escapeRegex(pageSpec.navigation_label)}$`);
  const button = page.getByRole("button", { name: accessibleName });
  if (await button.count() !== 1) throw new Error(`Navigation control was not unique: ${pageSpec.navigation_label}`);
  await button.click();
  await page.waitForTimeout(300);
}

async function waitForBodyRegex(page, expression, seconds) {
  const pattern = regexFromRule(expression);
  await page.waitForFunction(({ source, flags }) => new RegExp(source, flags).test(document.body.innerText), { source: pattern.source, flags: pattern.flags }, { timeout: (seconds || 10) * 1000 });
}

async function runSpecialTrigger(page, trigger) {
  if (!trigger) return;
  if (trigger.kind === "click") {
    const pattern = regexFromRule(trigger.label_regex);
    const locator = page.getByRole(trigger.role, { name: pattern }).first();
    if (await locator.count() !== 1) throw new Error(`Special-state trigger was not found: ${trigger.label_regex}`);
    await locator.click();
    return;
  }
  if (trigger.kind === "open_details") {
    const pattern = regexFromRule(trigger.label_regex);
    const summaries = page.locator("summary");
    const count = await summaries.count();
    for (let index = 0; index < count; index += 1) {
      const item = summaries.nth(index);
      if (pattern.test((await item.innerText()).trim())) {
        const open = await item.evaluate((element) => element.parentElement instanceof HTMLDetailsElement && element.parentElement.open);
        if (!open) await item.click();
        return;
      }
    }
    throw new Error(`Details trigger was not found: ${trigger.label_regex}`);
  }
  if (trigger.kind === "arm_selected_stt") {
    const consent = page.getByRole("checkbox", { name: /I approve this AssemblyAI cloud STT attempt/i });
    if (await consent.count() !== 1) throw new Error("Selected STT consent control was not found.");
    await consent.check();
    const arm = page.getByRole("button", { name: /arm.*selected|arm.*capture|arm/i }).first();
    if (await arm.count() !== 1) throw new Error("Selected STT Arm control was not found.");
    await arm.click();
    return;
  }
  throw new Error(`Unknown special-state trigger: ${trigger.kind}`);
}

async function exerciseControls(page, plan, pageId, result, seen) {
  const forbidden = plan.forbidden_enabled_controls.map(regexFromRule);
  let probe = await pageProbe(page);
  for (const control of probe.controls) {
    const identity = `${pageId}:${control.role}:${control.label}:${control.ordinal}`;
    if (seen.has(identity)) continue;
    seen.add(identity);
    const record = { identity, page_id: pageId, observed: control, started_at_utc: new Date().toISOString(), strategy: null, rule_id: null, outcome: null, error: null };
    if (control.disabled) {
      record.strategy = "observe_disabled";
      record.outcome = "disabled_observed";
      result.push(record);
      continue;
    }
    if (forbidden.some((pattern) => pattern.test(control.label))) {
      record.strategy = "forbidden_enabled";
      record.outcome = "failed";
      record.error = "Control forbidden by evidence policy was enabled.";
      result.push(record);
      continue;
    }
    const rule = plan.control_rules.find((candidate) => candidate.role === control.role && regexFromRule(candidate.label_regex).test(control.label));
    if (!rule) {
      record.strategy = "unmatched";
      record.outcome = "unexercised";
      result.push(record);
      continue;
    }
    record.strategy = rule.strategy;
    record.rule_id = rule.id;
    if (rule.strategy === "assert_disabled") {
      record.outcome = "failed";
      record.error = "Control required to remain disabled was enabled.";
      result.push(record);
      continue;
    }
    const locator = control.label
      ? page.getByRole(control.role, { name: control.label, exact: true }).first()
      : page.locator(`[data-installed-evidence-id="${control.evidence_id}"]`);
    try {
      if (rule.strategy === "edit_restore") {
        const original = await locator.inputValue();
        await locator.fill(original ? `${original} evidence-probe` : "evidence-probe");
        await locator.fill(original);
      } else if (rule.strategy === "select_restore") {
        const original = await locator.inputValue();
        const options = await locator.locator("option").evaluateAll((items) => items.map((item) => item.value).filter(Boolean));
        const alternate = options.find((value) => value !== original);
        if (alternate) { await locator.selectOption(alternate); await locator.selectOption(original); }
        else record.error = "No alternate enabled option was available; current selection was provenance-captured.";
      } else if (rule.strategy === "press_restore") {
        await locator.click();
        await page.waitForTimeout(150);
        if (await locator.count()) await locator.click();
      } else if (rule.strategy === "navigate_restore") {
        await locator.click();
        await page.waitForTimeout(150);
        const restore = plan.retained_pages.find((item) => item.id === pageId);
        if (restore) await navigateTo(page, restore);
      } else if (rule.strategy === "press") {
        await locator.click();
        await page.waitForTimeout(250);
        const dialog = page.getByRole("dialog");
        if (await dialog.count()) {
          const cancel = dialog.getByRole("button", { name: /cancel|close|not now/i }).first();
          if (await cancel.count()) await cancel.click();
        }
      } else throw new Error(`Unknown control strategy: ${rule.strategy}`);
      record.outcome = record.error ? "observed_no_alternate" : "exercised";
    } catch (error) {
      record.outcome = "failed";
      record.error = String(error && error.message || error);
    }
    record.finished_at_utc = new Date().toISOString();
    result.push(record);
  }
}

async function main() {
  const args = parseArgs(process.argv);
  const output = path.resolve(args.output);
  const plan = JSON.parse(fs.readFileSync(args.plan, "utf8"));
  if (plan.schema !== "interactive-npcs-installed-evidence-plan/v1") throw new Error("Unexpected evidence plan schema.");
  fs.mkdirSync(path.join(output, "screenshots"), { recursive: true });
  const playwright = require(args["playwright-module"]);
  const browser = await connectWithRetry(playwright.chromium, args.endpoint);
  const page = browser.contexts().flatMap((context) => context.pages()).find((candidate) => !candidate.url().startsWith("devtools://"));
  if (!page) throw new Error("Installed app WebView page was not found.");
  const consoleEvents = [];
  page.on("console", (message) => consoleEvents.push({ at_utc: new Date().toISOString(), type: message.type(), text: message.text(), location: message.location() }));
  page.on("pageerror", (error) => consoleEvents.push({ at_utc: new Date().toISOString(), type: "pageerror", text: error.message, stack: error.stack || null }));
  const cdp = await page.context().newCDPSession(page);
  const screens = [];
  const controls = [];
  const seenControls = new Set();
  const specialStateFailures = [];
  for (const pageSpec of plan.retained_pages) {
    await navigateTo(page, pageSpec);
    for (const profile of plan.view_profiles) {
      await cdp.send("Emulation.setDeviceMetricsOverride", { width: profile.layout_width, height: profile.layout_height, deviceScaleFactor: profile.device_scale_factor, mobile: false, screenWidth: profile.layout_width, screenHeight: profile.layout_height });
      await page.waitForTimeout(200);
      await exerciseControls(page, plan, pageSpec.id, controls, seenControls);
      screens.push(await captureScreen(page, cdp, output, `${pageSpec.id}-${profile.id}`, plan.capture, { page_id: pageSpec.id, profile }));
    }
  }
  for (const state of plan.special_states) {
    try {
      if (state.query) {
        await page.evaluate((query) => { history.replaceState({}, "", query); location.reload(); }, state.query);
        await page.waitForLoadState("domcontentloaded");
      } else {
        const pageSpec = plan.retained_pages.find((item) => item.id === state.page);
        if (!pageSpec) throw new Error(`Special state names unknown page: ${state.page}`);
        await navigateTo(page, pageSpec);
      }
      if (state.trigger) {
        await runSpecialTrigger(page, state.trigger);
        controls.push({
          identity: `${state.id}:special-trigger:${state.trigger.kind}`,
          page_id: state.page || "onboarding",
          observed: { role: state.trigger.role || "workflow", label: state.trigger.label_regex || state.trigger.kind, disabled: false },
          started_at_utc: new Date().toISOString(),
          finished_at_utc: new Date().toISOString(),
          strategy: `special_${state.trigger.kind}`,
          rule_id: "special-state-workflow",
          outcome: "exercised",
          error: null
        });
      }
      if (state.capture_when_regex) await waitForBodyRegex(page, state.capture_when_regex, state.wait_seconds || 10);
      for (const profileId of state.view_profiles) {
        const profile = plan.view_profiles.find((item) => item.id === profileId);
        if (!profile) throw new Error(`Special state names unknown profile: ${profileId}`);
        await cdp.send("Emulation.setDeviceMetricsOverride", { width: profile.layout_width, height: profile.layout_height, deviceScaleFactor: profile.device_scale_factor, mobile: false, screenWidth: profile.layout_width, screenHeight: profile.layout_height });
        await page.waitForTimeout(150);
        screens.push(await captureScreen(page, cdp, output, `${state.id}-${profile.id}`, plan.capture, { page_id: state.page || "onboarding", state_id: state.id, profile, operator_checkpoint: state.operator_checkpoint || null }));
      }
      if (state.post_capture_trigger) {
        await runSpecialTrigger(page, state.post_capture_trigger);
        controls.push({
          identity: `${state.id}:special-post-trigger:${state.post_capture_trigger.kind}`,
          page_id: state.page || "onboarding",
          observed: { role: state.post_capture_trigger.role || "workflow", label: state.post_capture_trigger.label_regex || state.post_capture_trigger.kind, disabled: false },
          started_at_utc: new Date().toISOString(),
          finished_at_utc: new Date().toISOString(),
          strategy: `special_${state.post_capture_trigger.kind}`,
          rule_id: "special-state-workflow",
          outcome: "exercised",
          error: null
        });
      }
      if (state.query) {
        await page.evaluate(() => { history.replaceState({}, "", "?page=session"); location.reload(); });
        await page.waitForLoadState("domcontentloaded");
      }
    } catch (error) {
      specialStateFailures.push({ state_id: state.id, error: String(error && error.message || error), operator_checkpoint: state.operator_checkpoint || null });
    }
  }
  await cdp.send("Emulation.clearDeviceMetricsOverride");
  const failures = [
    ...screens.flatMap((screen) => screen.probe.horizontal_overflow ? [{ type: "horizontal_overflow", screen_id: screen.id }] : []),
    ...screens.flatMap((screen) => screen.probe.enabled_unlabeled_count ? [{ type: "enabled_unlabeled_controls", screen_id: screen.id, count: screen.probe.enabled_unlabeled_count }] : []),
    ...controls.filter((control) => ["failed", "unexercised"].includes(control.outcome)).map((control) => ({ type: `control_${control.outcome}`, identity: control.identity, error: control.error })),
    ...consoleEvents.filter((event) => ["error", "pageerror"].includes(event.type)).map((event) => ({ type: "webview_error", event })),
    ...specialStateFailures.map((failure) => ({ type: "special_state_not_captured", ...failure }))
  ];
  writeJson(path.join(output, "screens.json"), { schema: "interactive-npcs-installed-screens/v1", run_id: args["run-id"], screens });
  writeJson(path.join(output, "controls.json"), { schema: "interactive-npcs-installed-controls/v1", run_id: args["run-id"], controls, coverage: { observed: controls.length, exercised: controls.filter((item) => item.outcome === "exercised").length, disabled_observed: controls.filter((item) => item.outcome === "disabled_observed").length, failed_or_unexercised: controls.filter((item) => ["failed", "unexercised"].includes(item.outcome)).length } });
  writeJson(path.join(output, "webview-events.json"), { schema: "interactive-npcs-webview-events/v1", run_id: args["run-id"], events: consoleEvents });
  writeJson(path.join(output, "driver-result.json"), { schema: "interactive-npcs-installed-driver-result/v1", run_id: args["run-id"], status: failures.length ? "failed" : "passed", screen_count: screens.length, screenshot_count: screens.reduce((sum, item) => sum + 1 + item.closeups.length, 0), control_count: controls.length, failures });
  process.stdout.write(`${JSON.stringify({ status: failures.length ? "failed" : "passed", failures: failures.length, screens: screens.length }, null, 2)}\n`);
  process.exit(failures.length ? 2 : 0);
}

main().catch((error) => {
  process.stderr.write(`${error && error.stack || error}\n`);
  process.exit(1);
});
