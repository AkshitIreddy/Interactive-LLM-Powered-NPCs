#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");

let playwright;
try {
  playwright = require("playwright-core");
} catch (error) {
  const bundled = path.join(
    process.env.USERPROFILE,
    ".cache",
    "codex-runtimes",
    "codex-primary-runtime",
    "dependencies",
    "node",
    "node_modules",
    "playwright",
  );
  if (!fs.existsSync(bundled)) throw error;
  playwright = require(bundled);
}

const baseUrl = process.env.NPC2_AUDIT_URL || "http://127.0.0.1:1420";
const outputRoot = path.resolve(
  process.env.NPC2_PACK_AUDIT_OUTPUT || "artifacts/content-pack-ui-audit",
);

async function capture(browser, name, viewport) {
  const context = await browser.newContext({
    viewport,
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await page.goto(`${baseUrl}/?page=world`, { waitUntil: "networkidle" });
  const panel = page.locator(".content-pack-workspace");
  await panel.waitFor({ state: "visible" });
  await panel.scrollIntoViewIfNeeded();
  await panel.screenshot({ path: path.join(outputRoot, `${name}-panel.png`) });
  await page.screenshot({
    path: path.join(outputRoot, `${name}-viewport.png`),
  });
  const editor = page.locator(".character-content-editor");
  await editor.locator("summary").click();
  await editor.scrollIntoViewIfNeeded();
  await editor.screenshot({
    path: path.join(outputRoot, `${name}-character-editor.png`),
  });
  const mouthPack = page.locator(".character-mouth-pack");
  await mouthPack.locator("summary").click();
  await mouthPack.scrollIntoViewIfNeeded();
  await mouthPack.screenshot({
    path: path.join(outputRoot, `${name}-mouth-pack.png`),
  });
  const measurement = await page.evaluate(() => ({
    bodyScrollWidth: document.body.scrollWidth,
    bodyClientWidth: document.body.clientWidth,
    panel: (() => {
      const node = document.querySelector(".content-pack-workspace");
      if (!node) return null;
      const rect = node.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    })(),
    editor: (() => {
      const node = document.querySelector(".character-content-editor");
      if (!node) return null;
      const rect = node.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    })(),
    mouthPack: (() => {
      const node = document.querySelector(".character-mouth-pack");
      if (!node) return null;
      const rect = node.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    })(),
  }));
  await context.close();
  return { name, viewport, errors, measurement };
}

async function main() {
  fs.mkdirSync(outputRoot, { recursive: true });
  const browser = await playwright.chromium.launch({
    headless: true,
    channel: "msedge",
  });
  try {
    const results = [];
    results.push(await capture(browser, "wide", { width: 1440, height: 900 }));
    results.push(await capture(browser, "narrow", { width: 760, height: 900 }));
    results.push(await capture(browser, "compact", { width: 320, height: 900 }));
    fs.writeFileSync(
      path.join(outputRoot, "report.json"),
      `${JSON.stringify({ generatedAt: new Date().toISOString(), results }, null, 2)}\n`,
    );
    if (
      results.some(
        (result) =>
          result.errors.length > 0 ||
          result.measurement.bodyScrollWidth >
            result.measurement.bodyClientWidth,
      )
    ) {
      process.exitCode = 1;
    }
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
