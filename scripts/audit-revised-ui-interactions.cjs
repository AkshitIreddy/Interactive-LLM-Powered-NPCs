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
const outputPath = path.resolve(
  process.env.NPC2_AUDIT_INTERACTIONS ||
    "artifacts/revised-ui-audit/interaction-report.json",
);
const checks = [];

function check(surface, control, passed, evidence, classification = "local-ui") {
  checks.push({ surface, control, outcome: passed ? "pass" : "fail", evidence, classification });
}

async function open(browser, relativeUrl) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await page.goto(`${baseUrl}${relativeUrl}`, { waitUntil: "networkidle" });
  await page.locator(".product-shell").waitFor({ state: "visible" });
  return { context, page, errors };
}

async function main() {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  const launch = { headless: true };
  if (process.env.NPC2_CHROMIUM_EXECUTABLE) launch.executablePath = process.env.NPC2_CHROMIUM_EXECUTABLE;
  else if (process.platform === "win32") launch.channel = "msedge";
  const browser = await playwright.chromium.launch(launch);
  try {
    {
      const { context, page, errors } = await open(browser, "/?page=session");
      const routes = [
        ["Session", "session"],
        ["Games & characters", "world"],
        ["Voice & models", "voice"],
        ["Diagnostics", "diagnostics"],
        ["Settings & help", "settings"],
      ];
      for (const [label, expected] of routes) {
        await page.getByRole("button", { name: label, exact: true }).click();
        check("shell", label, new URL(page.url()).searchParams.get("page") === expected, page.url(), "navigation");
      }
      await page.getByRole("button", { name: "Open session deck" }).click();

      const source = page.locator(".conversation-source");
      await source.locator("summary").click();
      check("session", "Change conversation", await source.getAttribute("open") !== null, "Source choices revealed.");
      const synthetic = page.getByRole("button", { name: "Synthetic review game" });
      await synthetic.click();
      check("session", "Synthetic review game", (await synthetic.getAttribute("aria-pressed")) === "true", "Review source selected.");

      await page.getByRole("button", { name: "Typed message" }).click();
      const transcript = page.getByRole("textbox", { name: "Turn transcript" });
      await transcript.fill("Current UI interaction audit");
      check("session", "Typed input", (await transcript.inputValue()) === "Current UI interaction audit", "Textarea enabled and editable.");
      const send = page.getByRole("button", { name: /Send typed turn/ });
      checks.push({ surface: "session", control: "Send typed turn", outcome: (await send.isDisabled()) ? "blocked-as-designed" : "unexpected-enabled", evidence: "Browser preview has no native runtime.", classification: "native-only" });

      const subtitle = page.locator(".subtitle-control input").first();
      if (await subtitle.isEnabled()) {
        const before = await subtitle.isChecked();
        await subtitle.click();
        check("session", "Show delivered subtitles", (await subtitle.isChecked()) !== before, { before, after: await subtitle.isChecked() });
      } else {
        checks.push({ surface: "session", control: "Show delivered subtitles", outcome: "blocked-as-designed", evidence: "Subtitle state is character-scoped native persistence and the browser preview has no native runtime.", classification: "native-only" });
      }

      await page.getByRole("button", { name: "Push-to-talk · F8" }).click();
      check("session", "Push-to-talk mode", (await page.getByRole("button", { name: "Push-to-talk · F8" }).getAttribute("aria-pressed")) === "true", "PTT readiness shown.");
      for (const disclosure of ["Turn activity", "Provider route evidence"]) {
        const summary = page.getByText(new RegExp(`^${disclosure}`)).first();
        await summary.click();
        check("session", disclosure, (await summary.locator("xpath=..").getAttribute("open")) !== null, "Disclosure toggled.");
      }
      await page.getByRole("button", { name: "Setup" }).click();
      check("shell", "Setup", await page.getByRole("dialog").isVisible(), "Setup dialog opened.");
      await page.getByRole("button", { name: "Close setup" }).click();
      check("shell", "Close setup", (await page.getByRole("dialog").count()) === 0, "Dialog closed.");
      check("session", "Runtime console", errors.length === 0, errors, "runtime-observation");
      await context.close();
    }

    {
      const { context, page, errors } = await open(browser, "/?page=voice");
      const sectionNav = page.getByRole("navigation", { name: "Voice workspace" });
      for (const label of ["Accounts", "Microphone", "Local models", "Model loadout"]) {
        const button = sectionNav.getByRole("button", { name: label, exact: false });
        await button.click();
        check("voice", `${label} section`, (await button.getAttribute("aria-current")) === "page", "Selected section is the only visible workspace.", "section-navigation");
      }

      const scopes = [
        ["03 · CHARACTER OVERRIDE", "CHARACTER"],
        ["02 · GAME OVERRIDE", "GAME"],
        ["01 · BASE ROUTE", "GLOBAL"],
      ];
      for (const [label, evidence] of scopes) {
        const button = page.getByRole("button", { name: new RegExp(label) });
        await button.click();
        check("voice", `${evidence} scope`, (await button.getAttribute("aria-pressed")) === "true", "Scope selection changed.");
      }

      const clone = page.getByRole("button", { name: "Clone" });
      await clone.click();
      const name = page.getByRole("textbox", { name: "Loadout name" });
      await name.fill("Revised audit clone");
      check("voice", "Clone and rename", (await name.inputValue()) === "Revised audit clone", "Isolated browser-local draft changed.", "browser-local-persistence");

      for (const role of ["Speech recognition", "Character voice", "Memory embeddings", "Reply model"]) {
        const button = page.getByRole("button", { name: new RegExp(role) }).first();
        await button.click();
        check("voice", `${role} role`, (await button.getAttribute("aria-pressed")) === "true", "Role editor changed.", "role-navigation");
      }

      for (const summaryText of ["Optional visual routes", "Privacy, cost, and route details", "Manual retry routes", "Validate active inheritance"]) {
        const summary = page.getByText(new RegExp(`^${summaryText}`)).first();
        await summary.click();
        check("voice", summaryText, (await summary.locator("xpath=..").getAttribute("open")) !== null, "Disclosure opened.");
      }

      await page.getByRole("button", { name: /^Connect or check / }).click();
      const accountsButton = sectionNav.getByRole("button", { name: "Accounts", exact: false });
      check("voice", "Connect or check provider", (await accountsButton.getAttribute("aria-current")) === "page", "Routed to Accounts without opening a browser credential prompt.", "section-navigation");
      const provider = page.getByRole("combobox", { name: "Provider account" });
      await provider.selectOption("elevenlabs");
      check("voice", "Provider account selector", (await provider.inputValue()) === "elevenlabs", "Account detail changed locally.");
      checks.push({ surface: "voice", control: "Connect account", outcome: (await page.getByRole("button", { name: "Connect account" }).isDisabled()) ? "blocked-as-designed" : "unexpected-enabled", evidence: "Secure credential prompt requires the native Windows app.", classification: "native-only" });

      check("voice", "Runtime console", errors.length === 0, errors, "runtime-observation");
      await context.close();
    }
  } finally {
    await browser.close();
  }

  const failures = checks.filter((item) => item.outcome === "fail" || item.outcome === "unexpected-enabled");
  const report = {
    schema: "npc.revised-ui-interaction-audit/v1",
    generatedAtUtc: new Date().toISOString(),
    baseUrl,
    note: "Headless browser-preview interaction audit in isolated contexts; no native mutation was attempted.",
    summary: {
      total: checks.length,
      failures: failures.length,
      blockedAsDesigned: checks.filter((item) => item.outcome === "blocked-as-designed").length,
    },
    checks,
  };
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify(report.summary, null, 2)}\n`);
  if (failures.length) process.exitCode = 1;
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
