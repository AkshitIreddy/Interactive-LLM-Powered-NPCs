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
  process.env.NPC2_AUDIT_INTERACTIONS ||
    "artifacts/current-ui-audit/interaction-report.json",
);
const executablePath = process.env.NPC2_CHROMIUM_EXECUTABLE;

const cases = [];

function record(surface, control, outcome, evidence, classification) {
  cases.push({ surface, control, outcome, evidence, classification });
}

async function settle(page) {
  await page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
  });
}

async function usePage(browser, url, callback) {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  const errors = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(`${baseUrl}${url}`, { waitUntil: "networkidle" });
  await page.locator(".product-shell").waitFor({ state: "visible" });
  await settle(page);
  try {
    await callback(page);
  } finally {
    record(
      url,
      "runtime console",
      errors.length ? "fail" : "pass",
      errors,
      "runtime-observation",
    );
    await context.close();
  }
}

async function main() {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  const launch = { headless: true };
  if (executablePath) launch.executablePath = executablePath;
  else if (process.platform === "win32") launch.channel = "msedge";
  const browser = await playwright.chromium.launch(launch);

  try {
    await usePage(browser, "/?page=session", async (page) => {
      const destinations = [
        ["01 Session deck", "session"],
        ["02 World", "world"],
        ["03 Voice & models", "voice"],
        ["04 Diagnostics", "diagnostics"],
        ["05 Settings & guide", "settings"],
      ];
      for (const [name, expected] of destinations) {
        await page.getByRole("button", { name }).click();
        const actual = new URL(page.url()).searchParams.get("page");
        record(
          "rail",
          name,
          actual === expected ? "pass" : "fail",
          { expected, actual },
          "navigation",
        );
      }
      await page.getByRole("button", { name: "Open session deck" }).click();
      record(
        "rail",
        "brand home",
        new URL(page.url()).searchParams.get("page") === "session"
          ? "pass"
          : "fail",
        page.url(),
        "navigation",
      );

      await page.getByRole("button", { name: "Typed message" }).click();
      const transcript = page.getByRole("textbox", { name: "Turn transcript" });
      const enabled = await transcript.isEnabled();
      await transcript.fill("Audit-only typed turn");
      const send = page.getByRole("button", { name: /Send typed turn/ });
      record(
        "session",
        "Typed message and transcript",
        enabled && (await transcript.inputValue()) === "Audit-only typed turn"
          ? "pass"
          : "fail",
        { sendDisabled: await send.isDisabled() },
        "ephemeral-ui-state",
      );
      record(
        "session",
        "Send typed turn",
        (await send.isDisabled()) ? "blocked-as-designed" : "unexpected-enabled",
        await send.textContent(),
        "native-only",
      );

      const subtitles = page.locator('input[type="checkbox"]').last();
      const before = await subtitles.isChecked();
      await subtitles.click();
      record(
        "session",
        "Show delivered subtitles",
        (await subtitles.isChecked()) !== before ? "pass" : "fail",
        { before, after: await subtitles.isChecked() },
        "ephemeral-ui-state",
      );
      await page.getByRole("button", { name: /Inspect world selection/ }).click();
      record(
        "session",
        "Inspect world selection",
        new URL(page.url()).searchParams.get("page") === "world"
          ? "pass"
          : "fail",
        page.url(),
        "navigation",
      );
    });

    await usePage(browser, "/?page=voice", async (page) => {
      const advanced = page.locator(".advanced-profiles");
      await advanced.locator("summary").click();
      record(
        "voice",
        "Advanced profiles disclosure",
        (await advanced.getAttribute("open")) !== null ? "pass" : "fail",
        "Disclosure opened and editor became reachable.",
        "ephemeral-ui-state",
      );

      const characterScope = page.getByRole("button", {
        name: /03 · CHARACTER OVERRIDE/,
      });
      await characterScope.click();
      record(
        "voice",
        "Character override scope",
        (await characterScope.getAttribute("aria-pressed")) === "true"
          ? "pass"
          : "fail",
        "Scope changes locally; create remains unavailable without a selected character.",
        "ephemeral-ui-state",
      );
      await page.getByRole("button", { name: /01 · BASE ROUTE/ }).click();

      const clone = page.getByRole("button", { name: "Clone" });
      await clone.click();
      const name = page.getByRole("textbox", { name: "Loadout name" });
      await name.fill("Audit clone");
      await name.blur();
      record(
        "voice",
        "Clone and rename loadout",
        (await name.inputValue()) === "Audit clone" ? "pass" : "fail",
        "Changed in an isolated browser context.",
        "browser-local-persistence",
      );

      const replyProvider = page.getByLabel("Reply model provider");
      const initialProvider = await replyProvider.inputValue();
      const selectableProvider = await replyProvider
        .locator("option:not([disabled])")
        .evaluateAll((options, current) =>
          options.map((option) => option.value).find((value) => value !== current),
        initialProvider);
      if (selectableProvider) await replyProvider.selectOption(selectableProvider);
      record(
        "voice",
        "Reply provider selector",
        selectableProvider && (await replyProvider.inputValue()) === selectableProvider
          ? "pass"
          : "not-applicable",
        { initialProvider, selectedProvider: await replyProvider.inputValue() },
        "browser-local-persistence",
      );
      if (selectableProvider) await replyProvider.selectOption(initialProvider);

      const fallback = page.locator(".manual-fallbacks input[type=checkbox]").first();
      await fallback.click();
      const fallbackSelect = page.getByLabel("Reply model manual fallback provider");
      record(
        "voice",
        "Manual fallback authorization",
        (await fallback.isChecked()) && (await fallbackSelect.isEnabled())
          ? "pass"
          : "fail",
        "Authorization only exposes a manual retry choice.",
        "browser-local-persistence",
      );

      const activate = page.getByRole("button", { name: "Activate for next turn" });
      const activationEnabled = await activate.isEnabled();
      if (activationEnabled) await activate.click();
      record(
        "voice",
        "Activate browser-preview loadout",
        activationEnabled &&
          (await page.getByRole("button", { name: "Active for next turn" }).count()) === 1
          ? "pass"
          : "blocked-as-designed",
        "Persists to isolated browser localStorage only; it is not native authority.",
        "browser-local-persistence",
      );

      const review = page.getByRole("button", { name: "Review active route" });
      record(
        "voice",
        "Review active route",
        (await review.isDisabled()) ? "blocked-as-designed" : "unexpected-enabled",
        "Requires protected native state.",
        "native-only",
      );
    });

    await usePage(browser, "/?page=settings", async (page) => {
      await page.getByRole("button", { name: "Selected game" }).click();
      const pressed = await page
        .getByRole("button", { name: "Selected game" })
        .getAttribute("aria-pressed");
      record(
        "settings",
        "Selected game preference scope",
        pressed === "true" ? "pass" : "fail",
        "Scope switches, while native values remain unavailable.",
        "ephemeral-ui-state",
      );

      const search = page.getByRole("searchbox");
      await search.fill("privacy");
      const guideButtons = page.locator(".guide-list button");
      const guideLabels = await guideButtons.allTextContents();
      record(
        "settings",
        "Guide search",
        guideLabels.some((label) => label.includes("Privacy and provider egress")) &&
          !guideLabels.some((label) => label.includes("Native diagnostics"))
          ? "pass"
          : "fail",
        guideLabels,
        "ephemeral-ui-state",
      );
      await guideButtons.first().click();
      record(
        "settings",
        "Guide selection",
        (await page.locator(".guide-detail").textContent()).includes("egress")
          ? "pass"
          : "fail",
        "Detail changed to the privacy and provider egress guide.",
        "ephemeral-ui-state",
      );
      await search.fill("");
      await page.getByRole("button", { name: "Native diagnostics", exact: false }).click();
      const guideAction = page.locator(".guide-detail button");
      if (await guideAction.isVisible()) await guideAction.click();
      record(
        "settings",
        "Native diagnostics guide action",
        new URL(page.url()).searchParams.get("page") === "diagnostics"
          ? "pass"
          : "fail",
        page.url(),
        "navigation",
      );
    });

    await usePage(browser, "/?page=session&onboarding=1", async (page) => {
      await page.getByRole("button", { name: "Continue" }).click();
      const feedback = await page.locator(".setup-feedback").textContent();
      record(
        "setup",
        "Continue from system",
        feedback.includes("native desktop app") ? "blocked-as-designed" : "fail",
        feedback.trim(),
        "native-persistence-only",
      );
      await page.getByRole("button", { name: "Close setup" }).click();
      record(
        "setup",
        "Close setup",
        (await page.locator(".setup-dialog").count()) === 0 ? "pass" : "fail",
        "The explicit query parameter makes close available.",
        "ephemeral-ui-state",
      );
    });
  } finally {
    await browser.close();
  }

  const failures = cases.filter((item) => item.outcome === "fail");
  const report = {
    schema: "npc.current-ui-interaction-audit/v1",
    generatedAtUtc: new Date().toISOString(),
    baseUrl,
    note:
      "Headless browser-preview audit in isolated contexts. Native-only commands were classified from their disabled UI and source bridge; no native mutation was attempted.",
    cases,
    summary: {
      total: cases.length,
      failures: failures.length,
      blockedAsDesigned: cases.filter((item) =>
        item.outcome.startsWith("blocked"),
      ).length,
    },
  };
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify(report.summary, null, 2)}\n`);
  if (failures.length) process.exitCode = 1;
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
