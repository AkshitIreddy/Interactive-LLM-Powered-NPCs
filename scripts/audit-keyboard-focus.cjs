#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");

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
const outputPath = path.resolve(
  process.env.NPC2_FOCUS_OUTPUT || "artifacts/current-ui-focus-audit.json",
);
const viewport = {
  width: Number(process.env.NPC2_AUDIT_WIDTH || 1440),
  height: Number(process.env.NPC2_AUDIT_HEIGHT || 900),
};
const zoom = Number(process.env.NPC2_AUDIT_ZOOM || 1);
const cssViewport = {
  width: Math.round(viewport.width / zoom),
  height: Math.round(viewport.height / zoom),
};
const surfaces = [
  { id: "session", url: "/?page=session" },
  { id: "world", url: "/?page=world" },
  { id: "voice", url: "/?page=voice" },
  { id: "diagnostics", url: "/?page=diagnostics" },
  { id: "settings", url: "/?page=settings" },
  { id: "setup-system", url: "/?page=session&onboarding=1", modal: true },
];

function labelOf(element) {
  return (
    element.getAttribute("aria-label") ||
    element.getAttribute("title") ||
    element.innerText ||
    element.value ||
    element.tagName.toLowerCase()
  )
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 160);
}

async function main() {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
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
      await page.emulateMedia({ reducedMotion: "reduce" });
      const consoleErrors = [];
      const pageErrors = [];
      page.on("console", (message) => {
        if (message.type() === "error") consoleErrors.push(message.text());
      });
      page.on("pageerror", (error) => pageErrors.push(error.message));
      await page.goto(`${baseUrl}${surface.url}`, { waitUntil: "networkidle" });
      await page.locator(".product-shell").waitFor({ state: "visible" });
      await page.evaluate(async () => {
        await document.fonts.ready;
        document.documentElement.classList.add("force-reduced-motion");
        await new Promise((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(resolve)),
        );
      });

      const expected = await page.evaluate(({ modal }) => {
        const selector =
          "button,input:not([type='hidden']),select,textarea,a[href],summary,[tabindex]";
        const dialog = document.querySelector(
          ".setup-dialog,[role='dialog'][aria-modal='true']",
        );
        return [...document.querySelectorAll(selector)]
          .filter((node) => {
            const element = /** @type {HTMLElement} */ (node);
            const input = /** @type {HTMLInputElement} */ (node);
            const style = getComputedStyle(element);
            const rect = element.getBoundingClientRect();
            const closedDetails = element.closest("details:not([open])");
            const hiddenByDetails = Boolean(
              closedDetails && element.tagName !== "SUMMARY",
            );
            return (
              !input.disabled &&
              element.tabIndex >= 0 &&
              element.getAttribute("aria-hidden") !== "true" &&
              style.display !== "none" &&
              style.visibility !== "hidden" &&
              rect.width > 0 &&
              rect.height > 0 &&
              !hiddenByDetails &&
              (!modal || !dialog || dialog.contains(element))
            );
          })
          .map((element, index) => {
            const id = `focus-${index}`;
            element.setAttribute("data-audit-focus-id", id);
            return {
              id,
              tag: element.tagName.toLowerCase(),
              label:
                element.getAttribute("aria-label") ||
                element.getAttribute("title") ||
                element.innerText.replace(/\s+/g, " ").trim().slice(0, 160) ||
                element.value ||
                element.tagName.toLowerCase(),
            };
          });
      }, { modal: surface.modal === true });

      await page.evaluate(() => {
        const active = /** @type {HTMLElement | null} */ (document.activeElement);
        active?.blur?.();
        document.documentElement.scrollTop = 0;
        document.body.scrollTop = 0;
      });

      const sequence = [];
      const seen = new Set();
      const maximumTabs = Math.max(8, expected.length + 5);
      for (let index = 0; index < maximumTabs; index += 1) {
        await page.keyboard.press("Tab");
        const focus = await page.evaluate(() => {
          const element = /** @type {HTMLElement | null} */ (
            document.activeElement
          );
          if (!element || element === document.body) return null;
          const style = getComputedStyle(element);
          const rect = element.getBoundingClientRect();
          const dialog = document.querySelector(
            ".setup-dialog,[role='dialog'][aria-modal='true']",
          );
          const label = (
            element.getAttribute("aria-label") ||
            element.getAttribute("title") ||
            element.innerText ||
            /** @type {HTMLInputElement} */ (element).value ||
            element.tagName.toLowerCase()
          )
            .replace(/\s+/g, " ")
            .trim()
            .slice(0, 160);
          return {
            id: element.getAttribute("data-audit-focus-id"),
            tag: element.tagName.toLowerCase(),
            label,
            inDialog: Boolean(dialog?.contains(element)),
            inViewport:
              rect.bottom > 0 &&
              rect.right > 0 &&
              rect.top < innerHeight &&
              rect.left < innerWidth,
            focusVisible: element.matches(":focus-visible"),
            outlineStyle: style.outlineStyle,
            outlineWidth: style.outlineWidth,
            boxShadow: style.boxShadow,
          };
        });
        if (!focus) continue;
        if (sequence.length > 0 && focus.id === sequence[0].id) break;
        sequence.push(focus);
        if (focus.id) seen.add(focus.id);
      }

      const missing = expected.filter((item) => !seen.has(item.id));
      const outsideDialog = surface.modal
        ? sequence.filter((item) => !item.inDialog)
        : [];
      const invisibleFocus = sequence.filter((item) => !item.inViewport);
      const missingFocusIndicator = sequence.filter((item) => {
        const outlineVisible =
          item.outlineStyle !== "none" && item.outlineWidth !== "0px";
        return !item.focusVisible || (!outlineVisible && item.boxShadow === "none");
      });
      results.push({
        ...surface,
        expectedCount: expected.length,
        visitedCount: sequence.length,
        expected,
        sequence,
        missing,
        outsideDialog,
        invisibleFocus,
        missingFocusIndicator,
        consoleErrors,
        pageErrors,
      });
      await context.close();
    }
  } finally {
    await browser.close();
  }

  const failures = results.flatMap((result) => {
    const messages = [];
    if (result.missing.length)
      messages.push(`${result.id}: ${result.missing.length} controls missing from Tab order`);
    if (result.outsideDialog.length)
      messages.push(`${result.id}: focus escaped the modal dialog`);
    if (result.invisibleFocus.length)
      messages.push(`${result.id}: ${result.invisibleFocus.length} focused controls stayed offscreen`);
    if (result.missingFocusIndicator.length)
      messages.push(`${result.id}: ${result.missingFocusIndicator.length} controls lack a visible focus indicator`);
    if (result.consoleErrors.length || result.pageErrors.length)
      messages.push(`${result.id}: browser errors occurred`);
    return messages;
  });
  const report = {
    schema: "npc.keyboard-focus-audit/v1",
    generatedAtUtc: new Date().toISOString(),
    baseUrl,
    viewport,
    cssViewport,
    zoom,
    results,
    failures,
  };
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(
    `${JSON.stringify({ outputPath, surfaces: results.length, failures }, null, 2)}\n`,
  );
  if (failures.length) process.exitCode = 1;
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
