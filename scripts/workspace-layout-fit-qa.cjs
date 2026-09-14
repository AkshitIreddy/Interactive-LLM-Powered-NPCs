const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright-core");

const baseUrl = process.env.NPC_CONTROL_URL ?? "http://127.0.0.1:1426";
const outputDirectory =
  process.env.NPC_LAYOUT_QA_OUTPUT ??
  "E:/temp/InteractiveNPCs/ui-refinement-20260914/workspaces";

(async () => {
  fs.mkdirSync(outputDirectory, { recursive: true });
  const browser = await chromium.launch({ headless: true, channel: "msedge" });
  const results = [];
  try {
    for (const [width, height] of [
      [1440, 900],
      [1280, 720],
    ]) {
      for (const pageName of ["session", "world", "settings", "voice"]) {
        const page = await browser.newPage({
          viewport: { width, height },
          reducedMotion: "reduce",
        });
        const errors = [];
        page.on("pageerror", (error) => errors.push(error.message));
        await page.goto(`${baseUrl}/?page=${pageName}`, {
          waitUntil: "networkidle",
        });
        await page.locator(".product-main").waitFor();
        await page.screenshot({
          path: path.join(
            outputDirectory,
            `${pageName}-${width}x${height}.png`,
          ),
        });
        const metrics = await page.evaluate((currentPage) => {
          const dimensions = (element) => {
            if (!element) return null;
            const bounds = element.getBoundingClientRect();
            return {
              clientHeight: element.clientHeight,
              scrollHeight: element.scrollHeight,
              clientWidth: element.clientWidth,
              scrollWidth: element.scrollWidth,
              top: Math.round(bounds.top),
              bottom: Math.round(bounds.bottom),
            };
          };
          const main = document.querySelector(".product-main");
          const rail = document.querySelector(".signal-rail");
          const railBounds = rail?.getBoundingClientRect();
          const signalText = [
            ...document.querySelectorAll(".signal-node small, .signal-node b"),
          ].map((element) => {
            const bounds = element.getBoundingClientRect();
            return {
              text: element.textContent?.trim(),
              fontSize: Number.parseFloat(getComputedStyle(element).fontSize),
              top: Math.round(bounds.top),
              bottom: Math.round(bounds.bottom),
              clipped: Boolean(
                railBounds &&
                  (bounds.top < railBounds.top ||
                    bounds.bottom > railBounds.bottom),
              ),
            };
          });
          return {
            page: currentPage,
            body: dimensions(document.body),
            main: dimensions(main),
            primary:
              currentPage === "session"
                ? dimensions(document.querySelector(".primary-instrument"))
                : currentPage === "world"
                  ? dimensions(document.querySelector(".character-database"))
                  : dimensions(
                      document.querySelector(
                        ".workspace-section-content:not([hidden])",
                      ),
                    ),
            auxiliary:
              currentPage === "world"
                ? dimensions(document.querySelector(".native-target-workspace"))
                : null,
            signalText,
          };
        }, pageName);
        results.push({ width, height, errors, ...metrics });

        if (pageName === "world") {
          const practiceButton = page.getByRole("button", {
            name: /Practice environment details/i,
          });
          await practiceButton.focus();
          await practiceButton.click();
          const dialog = page.getByRole("dialog", {
            name: "Practice environment",
          });
          await dialog.waitFor();
          await page.screenshot({
            path: path.join(
              outputDirectory,
              `practice-dialog-${width}x${height}.png`,
            ),
          });
          await page.keyboard.press("Shift+Tab");
          if (
            !(await page.evaluate(() =>
              Boolean(document.activeElement?.closest('[role="dialog"]')),
            ))
          ) {
            errors.push("Practice dialog did not trap keyboard focus.");
          }
          await page.keyboard.press("Escape");
          if (
            !(await practiceButton.evaluate(
              (element) => element === document.activeElement,
            ))
          ) {
            errors.push("Practice dialog did not restore opener focus.");
          }
          await page.getByRole("button", { name: /Story & voice/i }).click();
          await page
            .getByRole("dialog", { name: /story and voice/i })
            .waitFor();
          await page.screenshot({
            path: path.join(
              outputDirectory,
              `character-dialog-${width}x${height}.png`,
            ),
          });
          await page.keyboard.press("Escape");
        }

        if (pageName === "settings") {
          await page.getByRole("button", { name: /^Audio devices/i }).click();
          await page
            .getByRole("heading", { name: "Playback destination" })
            .waitFor();
          await page.getByRole("button", { name: /^Preferences/i }).click();
          await page.getByRole("button", { name: /^Effective setup/i }).click();
          await page
            .getByRole("heading", { name: "What the next turn will use" })
            .waitFor();
        }

        if (pageName === "voice") {
          for (const tabName of ["Accounts", "Microphone", "Local models"]) {
            await page
              .getByRole("button", { name: new RegExp(`^${tabName}`, "i") })
              .click();
            await page.screenshot({
              path: path.join(
                outputDirectory,
                `voice-${tabName.toLowerCase().replace(/\s+/g, "-")}-${width}x${height}.png`,
              ),
            });
          }
        }
        await page.close();
      }
    }
    fs.writeFileSync(
      path.join(outputDirectory, "report.json"),
      JSON.stringify(results, null, 2),
    );
    const failures = results.filter(
      (result) =>
        result.errors.length > 0 ||
        !result.main ||
        result.main.scrollHeight > result.main.clientHeight + 1 ||
        result.main.scrollWidth > result.main.clientWidth + 1 ||
        result.body.scrollHeight > result.body.clientHeight + 1 ||
        result.body.scrollWidth > result.body.clientWidth + 1 ||
        (result.page === "session" &&
          result.primary.scrollHeight > result.primary.clientHeight + 1) ||
        result.signalText.some((item) => item.clipped || item.fontSize < 11),
    );
    console.log(JSON.stringify(results, null, 2));
    if (failures.length) {
      throw new Error(
        `Workspace containment failed for ${failures
          .map((result) => `${result.page}@${result.width}x${result.height}`)
          .join(", ")}`,
      );
    }
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
