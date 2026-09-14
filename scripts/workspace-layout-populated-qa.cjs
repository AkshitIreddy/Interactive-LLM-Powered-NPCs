// Explicit native-data fixture review. This renders production components with
// real checked-in Cyberpunk profile data; it is layout evidence, never a claim
// that the desktop runtime or a game was connected.
const fs = require("node:fs");
const path = require("node:path");
const ts = require("../apps/control/node_modules/typescript");
const { chromium } = require("playwright-core");

const baseUrl = process.env.NPC_CONTROL_URL ?? "http://127.0.0.1:1426";
const outputDirectory =
  process.env.NPC_LAYOUT_QA_OUTPUT ??
  "E:/temp/InteractiveNPCs/ui-refinement-20260914/workspaces";

function readPreferenceFixtures() {
  const source = fs.readFileSync(
    path.join(
      __dirname,
      "../apps/control/src/ProductPreferencesWorkspace.test.tsx",
    ),
    "utf8",
  );
  const fixtureSource = source.slice(
    source.indexOf("const effectiveValue"),
    source.indexOf('describe("native product preferences"'),
  );
  const compiled = ts.transpileModule(fixtureSource, {
    compilerOptions: {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.CommonJS,
      jsx: ts.JsxEmit.React,
    },
  }).outputText;
  return new Function(
    `${compiled}; return {preferences: preferenceSnapshot(), subtitles: subtitleSnapshot(), configuration: configurationSnapshot()};`,
  )();
}

function makeFixture() {
  const profile = JSON.parse(
    fs.readFileSync(
      path.join(__dirname, "../profiles/games/cyberpunk-2077/profile.json"),
      "utf8",
    ),
  );
  const selectedCharacterId = "misty-olszewski";
  const inspectionFor = (character) => ({
    schemaVersion: 1,
    gameProfileId: profile.id,
    gameDisplayName: profile.display_name,
    selectedCharacterId,
    character: {
      id: character.id,
      displayName: character.display_name,
      aliases: character.aliases ?? [],
      biography: character.biography ?? "",
      personality: character.personality ?? "",
      dialogueStyle: character.dialogue_style ?? "",
      styleExamples: character.style_examples ?? [],
      openingLines: character.opening_lines ?? [],
      backgroundNpc: Boolean(character.background_npc),
      promptRole: character.prompt?.role ?? "",
      promptObjectives: character.prompt?.objectives ?? [],
      promptConstraints: character.prompt?.constraints ?? [],
      knowledgeRefs: character.prompt?.knowledge_refs ?? [],
      voice: {
        description: character.voice?.description ?? "",
        locale: character.voice?.locale ?? "en-US",
        styleTags: character.voice?.style_tags ?? [],
        providerVoiceId: null,
        adapterId: null,
        catalogVersion: null,
        license: null,
        userOverrideAllowed: character.voice?.user_override_allowed !== false,
      },
      identity: {
        strategy: character.identity?.strategy ?? "explicit_selection",
        evidence: character.identity?.evidence ?? ["explicit_selection"],
        fallback: character.identity?.fallback ?? "explicit_selection",
        automaticFaceRecognitionClaimed: false,
      },
    },
    authoredKnowledge: [],
    provenance: [],
    deliveredMemory: [],
    memoryScope: {
      userId: "layout-fixture",
      profileId: profile.id,
      gameId: profile.id,
      characterId: character.id,
      sessionId: null,
      saveId: null,
      crossGameWideningAllowed: false,
    },
  });
  const inspections = Object.fromEntries(
    profile.characters.map((character) => [
      character.id,
      inspectionFor(character),
    ]),
  );
  return {
    bootstrap: {
      contractVersion: 1,
      appVersion: "2.0.0-layout-fixture",
      onboarding: {
        schemaVersion: 1,
        completed: true,
        currentStep: "ready",
        selectedGameId: profile.id,
        preferences: {
          execution: "cloud",
          performance: "balanced",
          subtitles: true,
          ptt: true,
          localOnly: false,
          screenPresence: false,
          diagnostics: true,
        },
        updatedAtEpochMs: 1,
      },
      onboardingPersistence: {
        health: "healthy",
        detail: "Explicit layout fixture.",
      },
      runtime: {
        state: "ready",
        connected: true,
        backend: "nativeRuntime",
        processId: 1201,
        restartCount: 0,
        recentFailureCount: 0,
        protocolVersion: "1",
        fixtureOnly: true,
        detail: "Layout fixture runtime.",
      },
      mediaBroker: {
        state: "ready",
        connected: true,
        processId: 1202,
        restartCount: 0,
        recentFailureCount: 0,
        protocolVersion: 1,
        fixtureOnly: true,
        brokerState: "ready",
        captureAvailable: true,
        overlayAvailable: true,
        captureAudioAvailable: true,
        renderAudioAvailable: true,
        detail: "Layout fixture media broker.",
      },
      providers: [],
      gameProfiles: [
        {
          id: profile.id,
          displayName: profile.display_name,
          wave: "stable",
          safety: "singlePlayerOnly",
          catalogState: "bundled",
          defaultFallback: "audioOnly",
        },
      ],
      models: [],
      capabilities: {},
    },
    catalog: {
      schemaVersion: 1,
      gameProfileId: profile.id,
      gameDisplayName: profile.display_name,
      selectedCharacterId,
      defaultCharacterId: selectedCharacterId,
      characters: profile.characters.map((character) => ({
        id: character.id,
        displayName: character.display_name,
        aliases: character.aliases ?? [],
        backgroundNpc: Boolean(character.background_npc),
        voiceDescription: character.voice?.description ?? null,
        identityStrategy: character.identity?.strategy ?? "explicit_selection",
      })),
      editableAuthoredData: true,
    },
    inspections,
    defaultInspection: inspections[selectedCharacterId],
    mouthPackState: {
      schemaVersion: 1,
      installed: [],
      enabled: [],
      detail: "No full packs installed in this layout fixture.",
    },
    ...readPreferenceFixtures(),
  };
}

async function openFixturePage(browser, pageName, viewport, fixture) {
  const page = await browser.newPage({ viewport, reducedMotion: "reduce" });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript((data) => {
    window.__TAURI_INTERNALS__ = {
      invoke: async (command, args) => {
        if (command === "bootstrap_snapshot") return data.bootstrap;
        if (command === "selected_game_target")
          return data.selectedTarget ?? null;
        if (command === "character_database_catalog") return data.catalog;
        if (command === "character_database_inspection") {
          const characterId = args?.request?.characterId;
          return data.inspections[characterId] ?? data.defaultInspection;
        }
        if (command === "character_mouth_pack_state") {
          return data.mouthPackState;
        }
        if (command === "product_preferences_snapshot") {
          return data.preferences;
        }
        if (command === "read_subtitle_preferences") return data.subtitles;
        if (command === "effective_configuration_snapshot") {
          return data.configuration;
        }
        return null;
      },
      transformCallback: () => 1,
      unregisterCallback: () => {},
    };
  }, fixture);
  await page.goto(`${baseUrl}/?page=${pageName}`, { waitUntil: "networkidle" });
  await page.locator(".product-main").waitFor();
  return { page, errors };
}

(async () => {
  fs.mkdirSync(outputDirectory, { recursive: true });
  const fixture = makeFixture();
  const browser = await chromium.launch({ headless: true, channel: "msedge" });
  const results = [];
  try {
    for (const viewport of [
      { width: 1440, height: 900 },
      { width: 1280, height: 720 },
    ]) {
      const world = await openFixturePage(browser, "world", viewport, fixture);
      await world.page
        .getByRole("heading", { name: "People you can configure" })
        .waitFor();
      await world.page
        .getByRole("button", { name: /Misty Olszewski/i })
        .waitFor();
      await world.page.screenshot({
        path: path.join(
          outputDirectory,
          `fixture-populated-world-${viewport.width}x${viewport.height}.png`,
        ),
      });
      const worldMetrics = await world.page.evaluate(() => {
        const dimensions = (element) =>
          element
            ? {
                clientHeight: element.clientHeight,
                scrollHeight: element.scrollHeight,
                clientWidth: element.clientWidth,
                scrollWidth: element.scrollWidth,
              }
            : null;
        const tools = [
          ...document.querySelectorAll(".character-tool-grid button"),
        ];
        return {
          page: "world",
          body: dimensions(document.body),
          main: dimensions(document.querySelector(".product-main")),
          database: dimensions(document.querySelector(".character-database")),
          roster: dimensions(document.querySelector(".native-character-list")),
          inspector: dimensions(
            document.querySelector(".character-native-record"),
          ),
          characterCount: document.querySelectorAll(
            ".native-character-list button",
          ).length,
          toolsVisible: tools.every((button) => {
            const bounds = button.getBoundingClientRect();
            return bounds.top >= 0 && bounds.bottom <= innerHeight;
          }),
        };
      });
      results.push({
        mode: "native-data-fixture",
        ...viewport,
        errors: world.errors,
        ...worldMetrics,
      });
      await world.page.getByRole("button", { name: /Story & voice/i }).click();
      const storyDialog = world.page.getByRole("dialog", {
        name: /Misty Olszewski · story and voice/i,
      });
      await storyDialog.waitFor();
      await world.page.screenshot({
        path: path.join(
          outputDirectory,
          `fixture-character-story-${viewport.width}x${viewport.height}.png`,
        ),
      });
      const storyMetrics = await storyDialog.evaluate((dialog) => {
        const body = dialog.querySelector(".workspace-dialog__body");
        const biography = dialog.querySelector(".facts dd");
        return {
          dialogBottom: Math.round(dialog.getBoundingClientRect().bottom),
          viewportHeight: innerHeight,
          bodyClientHeight: body?.clientHeight ?? null,
          bodyScrollHeight: body?.scrollHeight ?? null,
          biographyWhiteSpace: biography
            ? getComputedStyle(biography).whiteSpace
            : null,
        };
      });
      results.at(-1).story = storyMetrics;
      await world.page.keyboard.press("Escape");
      await world.page.close();

      if (viewport.width === 1280) {
        const selectedFixture = structuredClone(fixture);
        selectedFixture.selectedTarget = {
          schemaVersion: 1,
          gameProfileId: "cyberpunk-2077",
          target: {
            processId: 44,
            nativeWindow: 55,
            executableName: "Cyberpunk2077.exe",
            executablePathSha256: "a".repeat(64),
            title: "Cyberpunk 2077",
            foreground: true,
            clientWidth: 1920,
            clientHeight: 1080,
          },
          processInstanceBound: true,
          userConfirmedOfflineSinglePlayer: true,
          captureAuthorized: true,
          safetyState: "allowed",
          safetyDetail: "Connected native game target.",
        };
        const selectedWorld = await openFixturePage(
          browser,
          "world",
          viewport,
          selectedFixture,
        );
        await selectedWorld.page
          .getByRole("button", { name: "Select NPC on screen" })
          .waitFor();
        await selectedWorld.page.screenshot({
          path: path.join(
            outputDirectory,
            `fixture-selected-game-${viewport.width}x${viewport.height}.png`,
          ),
        });
        const selectedMetrics = await selectedWorld.page.evaluate(() => {
          const workspace = document.querySelector(".native-target-workspace");
          const check = [...workspace.querySelectorAll("button")].find(
            (button) => button.textContent.trim() === "Check game capture",
          );
          const select = [...workspace.querySelectorAll("button")].find(
            (button) => button.textContent.trim() === "Select NPC on screen",
          );
          const isVisible = (element) => {
            if (!element) return false;
            const elementBounds = element.getBoundingClientRect();
            const workspaceBounds = workspace.getBoundingClientRect();
            return (
              elementBounds.top >= workspaceBounds.top &&
              elementBounds.bottom <= workspaceBounds.bottom &&
              elementBounds.top >= 0 &&
              elementBounds.bottom <= innerHeight
            );
          };
          return {
            page: "world-selected-game",
            workspaceClientHeight: workspace.clientHeight,
            workspaceScrollHeight: workspace.scrollHeight,
            selectNpcVisible: isVisible(select),
            checkCaptureVisible: isVisible(check),
          };
        });
        results.push({
          mode: "native-selected-target-fixture",
          ...viewport,
          errors: selectedWorld.errors,
          ...selectedMetrics,
        });
        await selectedWorld.page.close();
      }

      const settings = await openFixturePage(
        browser,
        "settings",
        viewport,
        fixture,
      );
      await settings.page.getByLabel("Execution preset").waitFor();
      for (const group of ["Response", "Interaction", "Presence"]) {
        await settings.page
          .getByRole("button", { name: new RegExp(`^${group}`, "i") })
          .click();
        await settings.page.screenshot({
          path: path.join(
            outputDirectory,
            `fixture-settings-conversation-${group.toLowerCase()}-${viewport.width}x${viewport.height}.png`,
          ),
        });
        const groupMetrics = await settings.page.evaluate((activeGroup) => {
          const pane = document.querySelector(
            `.preference-overrides[data-conversation-group="${activeGroup.toLowerCase()}"]`,
          );
          return {
            group: activeGroup,
            clientHeight: pane?.clientHeight ?? null,
            scrollHeight: pane?.scrollHeight ?? null,
          };
        }, group);
        results.push({
          mode: "native-preference-group-fixture",
          ...viewport,
          errors: settings.errors,
          page: "settings",
          ...groupMetrics,
        });
      }
      for (const category of ["Conversation", "Subtitles", "Effective setup"]) {
        await settings.page
          .getByRole("button", { name: new RegExp(`^${category}`, "i") })
          .click();
        await settings.page.screenshot({
          path: path.join(
            outputDirectory,
            `fixture-settings-${category.toLowerCase().replace(/\s+/g, "-")}-${viewport.width}x${viewport.height}.png`,
          ),
        });
      }
      const settingsMetrics = await settings.page.evaluate(() => {
        const dimensions = (element) =>
          element
            ? {
                clientHeight: element.clientHeight,
                scrollHeight: element.scrollHeight,
                clientWidth: element.clientWidth,
                scrollWidth: element.scrollWidth,
              }
            : null;
        return {
          page: "settings",
          body: dimensions(document.body),
          main: dimensions(document.querySelector(".product-main")),
          section: dimensions(
            document.querySelector(".workspace-section-content:not([hidden])"),
          ),
          editor: dimensions(
            document.querySelector(".preference-editor-shell"),
          ),
        };
      });
      results.push({
        mode: "native-data-fixture",
        ...viewport,
        errors: settings.errors,
        ...settingsMetrics,
      });
      await settings.page.close();
    }
  } finally {
    await browser.close();
  }
  fs.writeFileSync(
    path.join(outputDirectory, "populated-report.json"),
    JSON.stringify(results, null, 2),
  );
  const failures = results.filter(
    (result) =>
      result.errors.length > 0 ||
      (result.mode === "native-data-fixture" &&
        (!result.main ||
          result.main.scrollHeight > result.main.clientHeight + 1 ||
          result.main.scrollWidth > result.main.clientWidth + 1 ||
          result.body.scrollHeight > result.body.clientHeight + 1 ||
          result.body.scrollWidth > result.body.clientWidth + 1)) ||
      (result.page === "world" &&
        (result.characterCount !== 37 ||
          !result.toolsVisible ||
          result.story.dialogBottom > result.story.viewportHeight ||
          result.story.biographyWhiteSpace !== "pre-line")) ||
      (result.page === "world-selected-game" &&
        (!result.selectNpcVisible || !result.checkCaptureVisible)) ||
      (result.mode === "native-preference-group-fixture" &&
        result.scrollHeight > result.clientHeight + 1),
  );
  console.log(JSON.stringify(results, null, 2));
  if (failures.length) {
    throw new Error(
      `Populated fixture layout failed for ${failures
        .map((result) => `${result.page}@${result.width}x${result.height}`)
        .join(", ")}`,
    );
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
