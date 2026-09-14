const fs = require("node:fs");
const { chromium } = require("playwright-core");
const out =
  process.env.NPC_LAYOUT_QA_OUTPUT ||
  "E:/temp/InteractiveNPCs/ui-refinement-20260914/secondary-loadout";
const base = process.env.NPC_CONTROL_URL || "http://127.0.0.1:1426";
(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ channel: "msedge", headless: true });
  const results = [];
  try {
    for (const [width, height] of [
      [1440, 900],
      [1280, 720],
    ]) {
      const page = await browser.newPage({
        viewport: { width, height },
        reducedMotion: "reduce",
      });
      const errors = [];
      page.on("pageerror", (e) => errors.push(e.message));
      await page.goto(base + "/?page=voice", { waitUntil: "networkidle" });
      const inspect = async (name) => {
        await page.screenshot({
          path: out + "/" + width + "-" + name.replaceAll(" ", "-") + ".png",
        });
        const metrics = await page.evaluate(() => {
          const main = document.querySelector(".product-main");
          return {
            height: main.clientHeight,
            scroll: main.scrollHeight,
            width: main.clientWidth,
            scrollWidth: main.scrollWidth,
          };
        });
        results.push({
          viewport: [width, height],
          name,
          metrics,
          errors: [...errors],
        });
        if (
          metrics.scroll > metrics.height + 1 ||
          metrics.scrollWidth > metrics.width + 1 ||
          errors.length
        )
          throw Error(name + " workspace overflow or browser error");
      };
      for (const name of ["Accounts", "Microphone", "Local models"]) {
        await page.getByRole("button", { name, exact: true }).click();
        await inspect(name);
      }
      for (const name of [
        "PC budget",
        "Downloads",
        "Benchmark",
        "Advanced packs",
        "Mouth tracking",
      ]) {
        await page
          .getByRole("navigation", { name: "Local model settings" })
          .getByRole("button", { name, exact: true })
          .click();
        await inspect(name);
      }
      await page
        .getByRole("button", { name: "Model downloads →", exact: true })
        .click();
      if (
        !(await page
          .getByRole("heading", { name: "Model downloads", exact: true })
          .isVisible())
      )
        throw Error("Tracking handoff did not open downloads");
      await page
        .getByRole("button", { name: "Execution preference", exact: true })
        .click();
      const dialog = page.getByRole("dialog", {
        name: "Execution preference",
        exact: true,
      });
      await dialog.waitFor();
      for (let i = 0; i < 8; i++) {
        await page.keyboard.press("Tab");
        if (
          !(await dialog.evaluate((el) => el.contains(document.activeElement)))
        )
          throw Error("Execution dialog focus escaped");
      }
      await page.keyboard.press("Escape");
      if (
        !(await page
          .getByRole("button", { name: "Execution preference", exact: true })
          .evaluate((el) => el === document.activeElement))
      )
        throw Error("Execution dialog did not restore focus");
      await page.close();
    }
  } finally {
    await browser.close();
    fs.writeFileSync(out + "/report.json", JSON.stringify(results, null, 2));
  }
  console.log(
    "Secondary loadout checks passed: " +
      results.length +
      " viewport/pane cases",
  );
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
