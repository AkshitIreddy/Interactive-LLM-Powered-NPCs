#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");

let playwright;
try {
  playwright = require("playwright-core");
} catch (firstError) {
  try {
    playwright = require("playwright");
  } catch {
    const bundled = process.env.USERPROFILE
      ? path.join(
          process.env.USERPROFILE,
          ".cache",
          "codex-runtimes",
          "codex-primary-runtime",
          "dependencies",
          "node",
          "node_modules",
          "playwright",
        )
      : null;
    if (!bundled || !fs.existsSync(bundled)) throw firstError;
    playwright = require(bundled);
  }
}

const output = path.resolve(
  process.env.NPC2_PRODUCT_VISUAL_OUTPUT || "artifacts/product-console-visual-qa",
);
const baseUrl = process.env.NPC2_UI_URL || "http://127.0.0.1:1420";

async function settle(page) {
  await page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
  });
}

async function captureLocator(page, selector, filename) {
  const locator = page.locator(selector).first();
  if (!(await locator.count()) || !(await locator.isVisible())) {
    throw new Error(`Visible locator missing: ${selector}`);
  }
  await locator.scrollIntoViewIfNeeded();
  await settle(page);
  await locator.screenshot({
    path: path.join(output, filename),
    animations: "disabled",
    caret: "hide",
  });
}

async function captureSurface(browser, surface) {
  const context = await browser.newContext({ viewport: surface.viewport });
  const page = await context.newPage();
  const diagnostics = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      diagnostics.push(`console:${message.type()}: ${message.text()}`);
    }
  });
  page.on("pageerror", (error) => diagnostics.push(`pageerror: ${error.message}`));
  page.on("requestfailed", (request) =>
    diagnostics.push(
      `requestfailed: ${request.url()} (${request.failure()?.errorText ?? "unknown"})`,
    ),
  );
  await page.goto(`${baseUrl}${surface.url}`, { waitUntil: "networkidle" });
  try {
    await page.locator(".product-shell").waitFor({
      state: "visible",
      timeout: 15_000,
    });
  } catch (error) {
    await page.screenshot({
      path: path.join(output, `${surface.id}-boot-failure.png`),
      fullPage: true,
    });
    const body = (await page.locator("body").innerText()).trim().slice(0, 2_000);
    throw new Error(
      [
        `Product shell did not boot for ${surface.id}.`,
        `URL: ${page.url()}`,
        `Body: ${body || "<empty>"}`,
        ...diagnostics,
        `Cause: ${error.message}`,
      ].join("\n"),
    );
  }
  if (surface.openAdvanced) {
    await page.getByText("Advanced profiles", { exact: true }).click();
    await page.locator(".loadout-console").waitFor({ state: "visible" });
  }
  await settle(page);
  await page.screenshot({
    path: path.join(output, `${surface.id}-viewport.png`),
    animations: "disabled",
    caret: "hide",
    fullPage: false,
  });
  for (const [index, selector] of surface.closeups.entries()) {
    await captureLocator(
      page,
      selector,
      `${surface.id}-closeup-${index + 1}.png`,
    );
  }
  const metrics = await page.evaluate(() => ({
    documentWidth: document.documentElement.scrollWidth,
    viewportWidth: window.innerWidth,
    horizontalOverflow:
      document.documentElement.scrollWidth > window.innerWidth + 1,
    title: document.querySelector("h1")?.textContent?.trim() ?? null,
  }));
  await context.close();
  return { id: surface.id, ...metrics, diagnostics };
}

async function main() {
  fs.mkdirSync(output, { recursive: true });
  const launchOptions = { headless: true };
  if (process.env.NPC2_CHROMIUM_EXECUTABLE) {
    launchOptions.executablePath = process.env.NPC2_CHROMIUM_EXECUTABLE;
  } else if (process.platform === "win32") {
    launchOptions.channel = "msedge";
  }
  const browser = await playwright.chromium.launch(launchOptions);
  try {
    const surfaces = [
      {
        id: "voice-wide",
        url: "/?page=voice",
        viewport: { width: 1440, height: 900 },
        closeups: [".page-heading", ".route-map", ".provider-grid", ".voice-layout"],
      },
      {
        id: "voice-advanced",
        url: "/?page=voice",
        viewport: { width: 1440, height: 900 },
        openAdvanced: true,
        closeups: [".advanced-profiles", ".loadout-console__header", ".loadout-role"],
      },
      {
        id: "voice-narrow",
        url: "/?page=voice",
        viewport: { width: 720, height: 900 },
        closeups: [".page-heading", ".provider-grid", ".voice-layout"],
      },
    ];
    const results = [];
    for (const surface of surfaces) {
      results.push(await captureSurface(browser, surface));
    }
    fs.writeFileSync(
      path.join(output, "report.json"),
      `${JSON.stringify({ capturedAt: new Date().toISOString(), results }, null, 2)}\n`,
    );
    process.stdout.write(`${JSON.stringify(results, null, 2)}\n`);
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
