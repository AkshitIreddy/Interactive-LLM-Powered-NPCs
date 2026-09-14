const fs = require("node:fs");
const { chromium } = require("playwright-core");
(async () => {
  const out = "E:/temp/InteractiveNPCs/ui-refinement-20260914/fit";
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ headless: true, channel: "msedge" });
  const results = [];
  try {
    for (const [w, h] of [
      [1440, 900],
      [1280, 720],
    ]) {
      const p = await browser.newPage({
        viewport: { width: w, height: h },
        reducedMotion: "reduce",
      });
      const errors = [];
      p.on("pageerror", (e) => errors.push(e.message));
      await p.goto("http://127.0.0.1:1426/?page=voice", {
        waitUntil: "networkidle",
      });
      await p.locator(".neural-map").waitFor();
      for (const [role, label] of [
        ["llm", "Reply model."],
        ["embeddings", "Memory embeddings."],
        ["vision", "Optional vision."],
        ["tts", "Character voice."],
        ["stt", "Speech recognition."],
        ["lipSync", "Optional lip-sync."],
      ]) {
        await p
          .getByRole("button", {
            name: new RegExp("^" + label.replace(".", "\\.")),
          })
          .click();
        await p.screenshot({ path: `${out}/loadout-${w}-${role}.png` });
        results.push(
          await p.evaluate(
            ({ role, errors }) => {
              const main = document.querySelector(".product-main"),
                bay = document.querySelector(".loadout-neural-bay"),
                editor = document.querySelector(".loadout-roles"),
                foot = document.querySelector(".loadout-editor__footer");
              return {
                width: innerWidth,
                height: innerHeight,
                role,
                errors,
                main: { h: main.clientHeight, scroll: main.scrollHeight },
                editor: { h: editor.clientHeight, scroll: editor.scrollHeight },
                bay: bay.getBoundingClientRect().toJSON(),
                foot: foot.getBoundingClientRect().toJSON(),
                roles: [...document.querySelectorAll(".neural-system")].map(
                  (n) => n.getBoundingClientRect().toJSON(),
                ),
              };
            },
            { role, errors },
          ),
        );
      }
      await p
        .getByRole("button", { name: "Manage loadouts", exact: true })
        .click();
      await p.getByRole("dialog", { name: "Manage loadouts" }).waitFor();
      await p.screenshot({ path: `${out}/manager-${w}.png` });
      await p.keyboard.press("Escape");
      if (
        !(await p
          .getByRole("button", { name: "Manage loadouts", exact: true })
          .evaluate((n) => n === document.activeElement))
      )
        throw Error("Manager did not restore keyboard focus");
      await p
        .getByRole("button", { name: "Advanced routing", exact: true })
        .click();
      await p.getByRole("dialog", { name: "Advanced routing" }).waitFor();
      await p.screenshot({ path: `${out}/advanced-${w}.png` });
      await p.close();
    }
    fs.writeFileSync(`${out}/report.json`, JSON.stringify(results, null, 2));
    console.log(
      JSON.stringify(
        results.map((r) => ({
          width: r.width,
          role: r.role,
          main: r.main,
          editor: r.editor,
          rolesContained: r.roles.every(
            (x) => x.top >= r.bay.top && x.bottom <= r.bay.bottom,
          ),
          footerVisible: r.foot.bottom <= r.height,
          errors: r.errors,
        })),
        null,
        2,
      ),
    );
    if (
      results.some(
        (r) =>
          r.main.scroll > r.main.h + 1 ||
          r.editor.scroll > r.editor.h + 1 ||
          r.foot.bottom > r.height ||
          r.roles.some((x) => x.top < r.bay.top || x.bottom > r.bay.bottom) ||
          r.errors.length,
      )
    )
      throw Error("Main workspace containment failed");
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
