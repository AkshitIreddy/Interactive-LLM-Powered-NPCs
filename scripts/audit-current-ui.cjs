#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

let playwright;
try {
  playwright = require("playwright-core");
} catch (error) {
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
  if (!bundled || !fs.existsSync(bundled)) throw error;
  playwright = require(bundled);
}

const baseUrl = process.env.NPC2_AUDIT_URL || "http://127.0.0.1:1420";
const outputRoot = path.resolve(
  process.env.NPC2_AUDIT_OUTPUT || "artifacts/current-ui-audit",
);
const viewport = {
  width: Number(process.env.NPC2_AUDIT_WIDTH || 1440),
  height: Number(process.env.NPC2_AUDIT_HEIGHT || 900),
};
const zoom = Number(process.env.NPC2_AUDIT_ZOOM || 1);
const forcedColors = process.env.NPC2_AUDIT_FORCED_COLORS === "1";
const cssViewport = {
  width: Math.round(viewport.width / zoom),
  height: Math.round(viewport.height / zoom),
};
const surfaceCatalog = [
  { id: "session", url: "/?page=session" },
  {
    id: "session-typed",
    url: "/?page=session",
    clickButton: "Typed message",
    closeupSelectors: [
      ".product-rail",
      ".product-topbar",
      ".signal-rail",
      ".primary-instrument",
      ".session-side",
    ],
    optIn: true,
  },
  { id: "world", url: "/?page=world" },
  {
    id: "voice",
    url: "/?page=voice",
    scrollPositions: [0, 794, 1200, 1800, 2407],
  },
  {
    id: "voice-advanced",
    url: "/?page=voice",
    openDetails: ".loadout-overview-details",
  },
  {
    id: "voice-accounts",
    url: "/?page=voice",
    clickButton: "Accounts",
    closeupSelectors: [
      ".product-rail",
      ".product-topbar",
      ".page-heading",
      ".workspace-section-nav",
      ".account-workspace",
    ],
    optIn: true,
  },
  {
    id: "voice-tts-stock",
    url: "/?page=voice",
    clickRoleButton: "Character voice",
    openDetails: ".custom-voice-id",
    closeupSelectors: [
      ".product-rail nav",
      ".product-topbar",
      ".workspace-section-nav",
      ".cyberware-anatomy",
      ".custom-voice-id",
    ],
    optIn: true,
  },
  {
    id: "voice-microphone",
    url: "/?page=voice",
    clickButton: "Microphone",
    optIn: true,
  },
  {
    id: "voice-local",
    url: "/?page=voice",
    clickButton: "Local models",
    optIn: true,
  },
  { id: "diagnostics", url: "/?page=diagnostics" },
  { id: "settings", url: "/?page=settings" },
  {
    id: "settings-devices",
    url: "/?page=settings",
    clickButton: "Audio devices",
    closeupSelectors: [
      ".product-rail",
      ".product-topbar",
      ".page-heading",
      ".workspace-section-nav",
      ".settings-output-panel",
    ],
    optIn: true,
  },
  {
    id: "settings-help",
    url: "/?page=settings",
    clickButton: "Help",
    optIn: true,
  },
  { id: "setup-system", url: "/?page=session&onboarding=1", setup: true },
];
const requestedSurfaces = new Set(
  (process.env.NPC2_AUDIT_SURFACES || "")
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean),
);
const surfaces = surfaceCatalog.filter((surface) =>
  requestedSurfaces.size
    ? requestedSurfaces.has(surface.id)
    : !surface.optIn,
);

const hash = (buffer) =>
  crypto.createHash("sha256").update(buffer).digest("hex");

async function settle(page) {
  await page.evaluate(async () => {
    await document.fonts.ready;
    document.documentElement.classList.add("force-reduced-motion");
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
  });
}

async function stableScreenshot(capture, outputPath) {
  let previous = await capture();
  for (let attempt = 0; attempt < 6; attempt += 1) {
    const current = await capture();
    if (current.equals(previous)) {
      fs.writeFileSync(outputPath, current);
      return { sha256: hash(current), bytes: current.length };
    }
    previous = current;
  }
  fs.writeFileSync(outputPath, previous);
  return {
    sha256: hash(previous),
    bytes: previous.length,
    warning: "pixels-did-not-stabilize",
  };
}

async function main() {
  fs.mkdirSync(outputRoot, { recursive: true });
  const launch = { headless: true };
  if (process.env.NPC2_CHROMIUM_EXECUTABLE) {
    launch.executablePath = process.env.NPC2_CHROMIUM_EXECUTABLE;
  } else if (process.platform === "win32") {
    launch.channel = "msedge";
  }
  const browser = await playwright.chromium.launch(launch);
  const results = [];

  try {
    for (const surface of surfaces) {
      const context = await browser.newContext({
        viewport: cssViewport,
        deviceScaleFactor: zoom,
      });
      const page = await context.newPage();
      await page.emulateMedia({
        reducedMotion: "reduce",
        ...(forcedColors ? { forcedColors: "active" } : {}),
      });
      const consoleErrors = [];
      const pageErrors = [];
      page.on("console", (message) => {
        if (message.type() === "error") consoleErrors.push(message.text());
      });
      page.on("pageerror", (error) => pageErrors.push(error.message));
      await page.goto(`${baseUrl}${surface.url}`, { waitUntil: "networkidle" });
      await page.locator(".product-shell").waitFor({ state: "visible" });
      if (surface.clickButton) {
        const workspaceNav = page.locator(".workspace-section-nav");
        const clickRoot = (await workspaceNav.count()) ? workspaceNav : page;
        await clickRoot
          .getByRole("button", { name: surface.clickButton, exact: false })
          .click();
      }
      if (surface.clickRoleButton) {
        await page
          .getByRole("button", { name: surface.clickRoleButton, exact: false })
          .first()
          .click();
      }
      if (surface.openDetails) {
        await page.locator(surface.openDetails).evaluate((element) => {
          element.open = true;
        });
      }
      await settle(page);

      const surfaceRoot = path.join(outputRoot, surface.id);
      fs.mkdirSync(surfaceRoot, { recursive: true });
      for (const entry of fs.readdirSync(surfaceRoot)) {
        if (entry.endsWith(".png")) fs.unlinkSync(path.join(surfaceRoot, entry));
      }
      const screenshots = [];
      const fullPath = path.join(surfaceRoot, "full.png");
      screenshots.push({
        kind: "full",
        path: fullPath,
        ...(await stableScreenshot(
          () => page.screenshot({ animations: "disabled", caret: "hide" }),
          fullPath,
        )),
      });

      if (surface.setup) {
        const selectors = [
          ".setup-dialog",
          ".setup-header",
          ".setup-progress",
          ".setup-content",
          ".setup-footer",
        ];
        for (let index = 0; index < selectors.length; index += 1) {
          const locator = page.locator(selectors[index]).first();
          const outputPath = path.join(
            surfaceRoot,
            `closeup-${index + 1}.png`,
          );
          screenshots.push({
            kind: "element",
            selector: selectors[index],
            path: outputPath,
            ...(await stableScreenshot(
              () => locator.screenshot({ animations: "disabled", caret: "hide" }),
              outputPath,
            )),
          });
        }
      } else if (surface.closeupSelectors) {
        for (let index = 0; index < surface.closeupSelectors.length; index += 1) {
          const selector = surface.closeupSelectors[index];
          const locator = page.locator(selector).first();
          const outputPath = path.join(surfaceRoot, `closeup-${index + 1}.png`);
          screenshots.push({
            kind: "element",
            selector,
            path: outputPath,
            ...(await stableScreenshot(
              () => locator.screenshot({ animations: "disabled", caret: "hide" }),
              outputPath,
            )),
          });
        }
      } else {
        const main = page.locator(".product-main");
        const box = await main.boundingBox();
        const dimensions = await main.evaluate((element) => ({
          scrollHeight: element.scrollHeight,
          clientHeight: element.clientHeight,
        }));
        const documentDimensions = await page.evaluate(() => ({
          scrollHeight: document.scrollingElement?.scrollHeight ?? 0,
          clientHeight: document.documentElement.clientHeight,
        }));
        const mainMaximum = Math.max(
          0,
          dimensions.scrollHeight - dimensions.clientHeight,
        );
        const documentMaximum = Math.max(
          0,
          documentDimensions.scrollHeight - documentDimensions.clientHeight,
        );
        const scrollDocument = mainMaximum === 0 && documentMaximum > 0;
        const maximum = scrollDocument ? documentMaximum : mainMaximum;
        let positions = surface.scrollPositions && !process.env.NPC2_AUDIT_AUTO_SCROLL
          ? [...new Set(surface.scrollPositions.map((value) => Math.min(maximum, value)))]
          : [...new Set([0, maximum * 0.33, maximum * 0.66, maximum])];
        if (positions.length < 4 && maximum > 0) {
          positions = [...new Set([0, maximum * 0.33, maximum * 0.66, maximum])];
        }
        const railPath = path.join(surfaceRoot, "closeup-1-rail.png");
        screenshots.push({
          kind: "element",
          selector: ".product-rail",
          path: railPath,
          ...(await stableScreenshot(
            () => page.locator(".product-rail").screenshot({ animations: "disabled" }),
            railPath,
          )),
        });
        let index = 2;
        for (const requestedTop of positions) {
          const actualTop = scrollDocument
            ? await page.evaluate((top) => {
                window.scrollTo({ top, behavior: "instant" });
                return window.scrollY;
              }, Math.round(requestedTop))
            : await main.evaluate((element, top) => {
                element.scrollTo({ top, behavior: "instant" });
                return element.scrollTop;
              }, Math.round(requestedTop));
          await settle(page);
          const outputPath = path.join(
            surfaceRoot,
            `closeup-${index}-main-${actualTop}.png`,
          );
          screenshots.push({
            kind: "viewport-clip",
            scrollTop: actualTop,
            path: outputPath,
            ...(await stableScreenshot(
              () =>
                page.screenshot(
                  scrollDocument
                    ? { animations: "disabled", caret: "hide" }
                    : {
                        animations: "disabled",
                        caret: "hide",
                        clip: box,
                      },
                ),
              outputPath,
            )),
          });
          index += 1;
        }
      }

      const controls = await page.locator("button,input,select,textarea,a[href],summary").evaluateAll(
        (nodes) =>
          nodes.map((node, index) => {
            const element = /** @type {HTMLElement} */ (node);
            const input = /** @type {HTMLInputElement} */ (node);
            const rect = element.getBoundingClientRect();
            const style = getComputedStyle(element);
            const closedDetails = element.closest("details:not([open])");
            const hiddenByClosedDetails = Boolean(
              closedDetails && !element.closest("summary"),
            );
            const blockingDialog = document.querySelector(
              ".setup-overlay, [role='dialog'][aria-modal='true']",
            );
            const blockedByDialog = Boolean(
              blockingDialog && !blockingDialog.contains(element),
            );
            const label =
              element.getAttribute("aria-label") ||
              element.getAttribute("title") ||
              element.innerText ||
              input.value ||
              "";
            return {
              index,
              tag: element.tagName.toLowerCase(),
              type: input.type || null,
              label: label.replace(/\s+/g, " ").trim(),
              disabled: Boolean(input.disabled),
              checked: "checked" in input ? Boolean(input.checked) : null,
              value: "value" in input ? String(input.value) : null,
              href: element.getAttribute("href"),
              visible:
                style.display !== "none" &&
                style.visibility !== "hidden" &&
                rect.width > 0 &&
                rect.height > 0 &&
                !hiddenByClosedDetails,
              blockedByDialog,
              reachable:
                !input.disabled &&
                style.display !== "none" &&
                style.visibility !== "hidden" &&
                rect.width > 0 &&
                rect.height > 0 &&
                !hiddenByClosedDetails &&
                !blockedByDialog,
              inViewport:
                rect.bottom > 0 &&
                rect.right > 0 &&
                rect.top < innerHeight &&
                rect.left < innerWidth,
            };
          }),
      );
      const layout = await page.evaluate(() => {
        const main = document.querySelector(".product-main");
        return {
          viewportWidth: document.documentElement.clientWidth,
          documentWidth: document.documentElement.scrollWidth,
          mainClientWidth: main?.clientWidth ?? null,
          mainScrollWidth: main?.scrollWidth ?? null,
          mainClientHeight: main?.clientHeight ?? null,
          mainScrollHeight: main?.scrollHeight ?? null,
          documentHeight: document.scrollingElement?.scrollHeight ?? null,
        };
      });
      results.push({
        ...surface,
        layout,
        horizontalOverflow:
          layout.documentWidth > layout.viewportWidth ||
          (layout.mainScrollWidth ?? 0) > (layout.mainClientWidth ?? 0),
        controls,
        consoleErrors,
        pageErrors,
        screenshots,
      });
      await context.close();
    }
  } finally {
    await browser.close();
  }

  const report = {
    schema: "npc.current-ui-functional-audit/v1",
    generatedAtUtc: new Date().toISOString(),
    baseUrl,
    viewport,
    cssViewport,
    zoom,
    forcedColors,
    note: "Browser-preview rendering; native-only commands are intentionally unavailable.",
    surfaces: results,
  };
  fs.writeFileSync(
    path.join(outputRoot, "report.json"),
    `${JSON.stringify(report, null, 2)}\n`,
  );
  process.stdout.write(
    `${JSON.stringify({ outputRoot, surfaces: results.length, errors: results.flatMap((item) => item.consoleErrors.concat(item.pageErrors)), overflow: results.filter((item) => item.horizontalOverflow).map((item) => item.id) }, null, 2)}\n`,
  );
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
