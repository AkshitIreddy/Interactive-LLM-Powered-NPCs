import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  discover: vi.fn(),
  readTarget: vi.fn(),
  selectTarget: vi.fn(),
  clearTarget: vi.fn(),
  verifyCapture: vi.fn(),
  actorStart: vi.fn(),
  actorStatus: vi.fn(),
  actorCancel: vi.fn(),
  catalog: vi.fn(),
  inspect: vi.fn(),
  selectCharacter: vi.fn(),
  memoryStatus: vi.fn(),
  backupMemory: vi.fn(),
  listBackups: vi.fn(),
  deleteBackup: vi.fn(),
  eraseMemory: vi.fn(),
  restoreBackup: vi.fn(),
  removeAllMemory: vi.fn(),
  correctEncounter: vi.fn(),
  mergeEncounters: vi.fn(),
  settings: vi.fn(),
  telemetry: vi.fn(),
  pack: vi.fn(),
  saveSettings: vi.fn(),
  mutatePack: vi.fn(),
  loadoutPlanner: vi.fn(),
  admitLoadout: vi.fn(),
  trustedCatalog: vi.fn(),
  optionalLifecycle: vi.fn(),
  mutateOptional: vi.fn(),
  cancelOptional: vi.fn(),
  activateOptional: vi.fn(),
  benchmarkStatus: vi.fn(),
  benchmarkStart: vi.fn(),
  benchmarkCancel: vi.fn(),
  benchmarkReport: vi.fn(),
  mouthPackState: vi.fn(),
}));

vi.mock("./tauriBridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./tauriBridge")>()),
  discoverGameTargets: bridge.discover,
  readSelectedGameTarget: bridge.readTarget,
  selectGameTarget: bridge.selectTarget,
  clearGameTarget: bridge.clearTarget,
  verifySelectedGameCapture: bridge.verifyCapture,
  startManualActorPicker: bridge.actorStart,
  readManualActorPickerStatus: bridge.actorStatus,
  cancelManualActorPicker: bridge.actorCancel,
  readCharacterDatabaseCatalog: bridge.catalog,
  inspectCharacterDatabase: bridge.inspect,
  persistSelectedCharacter: bridge.selectCharacter,
  readCharacterMemoryStatus: bridge.memoryStatus,
  backupAllLocalMemory: bridge.backupMemory,
  listLocalMemoryBackups: bridge.listBackups,
  deleteLocalMemoryBackup: bridge.deleteBackup,
  eraseCharacterMemory: bridge.eraseMemory,
  restoreLocalMemoryBackup: bridge.restoreBackup,
  removeAllLocalMemory: bridge.removeAllMemory,
  correctEncounterToAuthoredCharacter: bridge.correctEncounter,
  mergeUnknownEncounters: bridge.mergeEncounters,
  readLocalResourceSettings: bridge.settings,
  readLocalResourceTelemetry: bridge.telemetry,
  readExperimentalVisualPackStatus: bridge.pack,
  saveLocalResourceSettings: bridge.saveSettings,
  mutateExperimentalVisualPack: bridge.mutatePack,
  readSelectedLocalLoadoutPlanner: bridge.loadoutPlanner,
  admitSelectedLocalLoadout: bridge.admitLoadout,
  readTrustedLocalPackCatalog: bridge.trustedCatalog,
  readTrustedOptionalPackLifecycle: bridge.optionalLifecycle,
  mutateTrustedOptionalPack: bridge.mutateOptional,
  cancelTrustedOptionalPackDownload: bridge.cancelOptional,
  activateTrustedOptionalPack: bridge.activateOptional,
  readThisPcBenchmarkStatus: bridge.benchmarkStatus,
  startThisPcBenchmark: bridge.benchmarkStart,
  cancelThisPcBenchmark: bridge.benchmarkCancel,
  readThisPcBenchmarkReport: bridge.benchmarkReport,
}));

vi.mock("./characterMouthPacks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./characterMouthPacks")>()),
  readCharacterMouthPackState: bridge.mouthPackState,
}));

import {
  CharacterDatabase,
  EncounterLifecycleControls,
  GameTargetWorkspace,
  LocalResourcePlanner,
  ThisPcBenchmark,
} from "./ProductWorkspaces";

const settings = {
  schemaVersion: 1,
  governor: {
    schema: "npc.resource-governor-policy/v1",
    vram_soft_ceiling_basis_points: 9000,
    ram_soft_ceiling_basis_points: 8500,
    minimum_vram_safety_bytes: 1_500_000_000,
    proportional_vram_safety_basis_points: 1500,
    minimum_ram_safety_bytes: 2_000_000_000,
    maximum_snapshot_age_millis: 2000,
    keep_warm_millis: 30000,
    unload_ttl_millis: 120000,
  },
  gameReserveVramBytes: 4 * 1024 ** 3,
  gameAdditionalReserveRamBytes: 2 * 1024 ** 3,
  preferredResidency: "cpu_resident_gpu_cold" as const,
};
const unavailable = {
  availability: "unavailable" as const,
  reason: "game_process_not_selected",
  provenance: {
    captured_unix_millis: 1,
    captured_monotonic_millis: 1,
    source: "unsupported_platform",
  },
};
const available = (value: number) => ({
  availability: "available" as const,
  value,
  provenance: {
    captured_unix_millis: 1,
    captured_monotonic_millis: 1,
    source: "dxgi_process_video_memory_info",
  },
});
const telemetry = {
  settings,
  admissionReady: false,
  admissionDetail:
    "Local activation is fail-closed because telemetry is incomplete.",
  snapshot: {
    schema: "npc.system-telemetry/resource-snapshot-v1",
    captured_unix_millis: 1,
    captured_monotonic_millis: 1,
    selected_game_pid: null,
    physical_ram_bytes: available(16 * 1024 ** 3),
    available_ram_bytes: available(8 * 1024 ** 3),
    adapter: {
      availability: "available" as const,
      value: {
        description: "Test GPU",
        luid: { low_part: 1, high_part: 2 },
        vendor_id: 1,
        device_id: 2,
        subsystem_id: 3,
        revision: 4,
      },
      provenance: {
        captured_unix_millis: 1,
        captured_monotonic_millis: 1,
        source: "dxgi_adapter_description",
      },
    },
    device_fingerprint_sha256: { ...unavailable, reason: "adapter_not_found" },
    dedicated_vram_bytes: available(12 * 1024 ** 3),
    os_local_vram_budget_bytes: available(10 * 1024 ** 3),
    current_process_local_vram_bytes: available(1024 ** 3),
    total_device_pressure_vram_bytes: available(2 * 1024 ** 3),
    selected_game_working_set_bytes: unavailable,
    selected_game_vram_bytes: unavailable,
  },
};
const pack = {
  schemaVersion: 1,
  packId: "openseeface-visual-signal",
  revision: "1",
  phase: "not_installed" as const,
  installedArtifactSha256: [],
  trustDomain: "localReviewDevOnly",
  experimental: true,
  explicitDownloadRequired: true,
  completeLipSyncModel: false,
  detail: "Not installed. This is not a complete lip-sync model.",
};
const selectedLoadout = {
  selection_id: "selected-local-loadout-1",
  roles: [
    {
      role: "language_model" as const,
      identity: { pack_id: "local.llm.test", revision: "r1" },
      preferred_residency: "cpu_resident_gpu_cold" as const,
    },
  ],
  expected_idle_millis: 1_000,
};
const unavailableLoadoutPlanner = {
  schemaVersion: 1,
  ready: false,
  detail:
    "Release model catalog root is unavailable; admission is fail-closed.",
  selected: null,
  planner: null,
};
const trustedCatalog = {
  schemaVersion: 1,
  ready: true,
  detail: "Verified signed release catalog. Current-device fit is separate.",
  trustScope: "automated_local_review_bootstrap" as const,
  productionTrust: false,
  rotationRequiredBeforeRelease: true,
  promotionSupported: false,
  publicationSupported: false,
  packs: [
    {
      identity: { pack_id: "local.llm.qwen", revision: "r1" },
      display_name: "Qwen local conversation",
      description: "Compact local language model.",
      recommendation_reason:
        "CPU-first option when hosted text is unavailable.",
      capability: { kind: "language_model", scope: "generic" },
      runtime: "llama.cpp",
      runtime_revision: "b7000",
      abi: "gguf-v3",
      backends: ["cpu", "vulkan", "cuda"],
      exact_artifact_download_bytes: 2 * 1024 ** 3,
      installed_bytes: 2.1 * 1024 ** 3,
      planning_storage_bytes: 2.2 * 1024 ** 3,
      planning_peak_install_bytes: 4.2 * 1024 ** 3,
      admission_state: "blocked_pending_measurement",
      admission_reason: "No signed current-device envelope.",
      allowed_residencies: ["cpu_resident_gpu_cold"],
      license: {
        spdx_expression: "Apache-2.0",
        license_name: "Apache License 2.0",
        license_url: "https://www.apache.org/licenses/LICENSE-2.0",
        redistributable: true,
        acceptance_required: false,
        component_ids: ["qwen"],
      },
      lifecycle: {
        explicit_download_required: true,
        automatic_download_allowed: false,
        install_strategy: "verify_then_atomic_activate",
        repair_strategy: "verify_quarantine_reinstall",
        remove_requires_unreferenced: true,
        activation_gates: ["current_device_measurement"],
      },
      non_qualifying_review_evidence_count: 2,
      qualified_envelope_count: 0,
      measurement_status: "unavailable",
      measurement_detail: "Review evidence is not admission evidence.",
      qualified_measurement: null,
    },
  ],
};
const optionalLifecycle = {
  schemaVersion: 1,
  ready: true,
  detail: "Verified catalog lifecycle is ready.",
  packs: [
    {
      identity: { pack_id: "local.llm.qwen", revision: "r1" },
      phase: "not_installed" as const,
      explicitDownloadRequired: true,
      automaticDownloadAllowed: false,
      licenseAcceptanceRequired: false,
      canInstall: true,
      canRepair: false,
      canRemove: false,
      detail: "Not installed. No silent download.",
    },
  ],
};
const idleBenchmark = {
  state: "idle" as const,
  reportId: null,
  requestedIterations: 0,
  completedIterations: 0,
  startedAtUtc: null,
  elapsedMillis: 0,
  reportFileName: null,
  unavailableComponents: [],
  actionCodes: [],
};
const benchmarkReport = {
  schemaVersion: "interactive-npcs-this-pc-benchmark-report/v1",
  reportId: "report-1",
  generatedAtUtc: "2026-08-30T10:00:00Z",
  state: "partial" as const,
  classification: {
    executionMode: "live" as const,
    measurementKind: "measured" as const,
    acceptanceEligible: false,
    reason: "Some native receipt producers are unavailable.",
  },
  bounds: {
    requestedIterations: 10,
    completedIterations: 4,
    timeoutMillis: 120000,
    baselineWindowMillis: 3000,
    elapsedMillis: 42000,
    completedWithinBounds: true,
  },
  binding: {
    gameProfileId: "skyrim-special-edition",
    executableSha256: "a".repeat(64),
    targetInstanceRecorded: false as const,
    loadoutRevision: "loadout-7",
    providerRoutes: [],
  },
  hardware: {
    operatingSystem: "windows",
    architecture: "x86_64",
    logicalProcessorCount: 16,
    adapterDescription: "Test GPU",
    adapterFingerprintSha256: null,
    physicalRamBytes: null,
    dedicatedVramBytes: null,
    hostnameRecorded: false as const,
    environmentVariablesRecorded: false as const,
  },
  provenance: {
    runtimeRevision: "runtime-1",
    brokerRevision: "broker-1",
    compositorRevision: "compositor-1",
    processLoadSamplerRevision: "process-1",
    gameFrameSamplerRevision: "frame-1",
    timingClock: "monotonic-nanoseconds",
    systemTelemetrySchema: "npc.system-telemetry/resource-snapshot-v1",
    providerPayloadsRecorded: false as const,
    promptsRecorded: false as const,
    transcriptsRecorded: false as const,
    audioRecorded: false as const,
    screenshotsRecorded: false as const,
    credentialsRecorded: false as const,
    filePathsRecorded: false as const,
  },
  coverage: {
    components: [
      { component: "live_llm_provider", availability: "ready" as const },
      {
        component: "game_frame_sampler",
        availability: "unavailable" as const,
        reason: "runtime_timing_receipt_unavailable",
        action: "review_diagnostics",
      },
    ],
    unavailableComponents: ["game_frame_sampler"],
    invalidReceiptCount: 0,
    failedIterationCount: 1,
  },
  metrics: [
    {
      metric: "llm_time_to_first_token_ms",
      unit: "ms",
      sampleCount: 4,
      minimum: 100,
      mean: 130,
      p50: 120,
      p95: 180,
      p99: 190,
      maximum: 200,
    },
  ],
  frameImpact: {
    baselineFrameTimeMs: null,
    activeFrameTimeMs: null,
    baselineFps: null,
    activeFps: null,
    p50FrameTimeDeltaMs: null,
    p95FrameTimeDeltaMs: null,
    p50FpsDelta: null,
    p50FpsImpactPercent: null,
  },
  persistence: {
    state: "persisted",
    reportFileName: "this-pc-benchmark-report-1.json",
    atomicWrite: true,
    reportDirectoryRecorded: false as const,
  },
};
const inspection = {
  schemaVersion: 1,
  gameProfileId: "skyrim-special-edition",
  gameDisplayName: "Skyrim Special Edition",
  selectedCharacterId: null,
  character: {
    id: "lydia",
    displayName: "Lydia",
    aliases: [],
    biography: "Housecarl.",
    personality: "Direct.",
    dialogueStyle: "Formal.",
    styleExamples: [],
    openingLines: [],
    backgroundNpc: false,
    promptRole: "Housecarl",
    promptObjectives: [],
    promptConstraints: [],
    knowledgeRefs: [],
    voice: {
      description: "Nord",
      locale: "en-US",
      styleTags: [],
      providerVoiceId: null,
      adapterId: null,
      catalogVersion: null,
      license: null,
      userOverrideAllowed: false,
    },
    identity: {
      strategy: "explicit",
      evidence: [],
      fallback: "background",
      automaticFaceRecognitionClaimed: false,
    },
  },
  authoredKnowledge: [],
  provenance: [],
  deliveredMemory: [
    {
      turnId: "stored-turn-hash-9f2c",
      speaker: "assistant",
      deliveredText: "I am sworn to carry your burdens.",
      contentSha256: "b".repeat(64),
      deliveredAtMs: 42,
      sequence: 3,
      providerId: "nvidia-nim",
      deliveryReceiptId: "receipt-17",
      provenanceSourceKind: "runtime_turn",
      provenanceSourceId: "request-turn-42",
    },
  ],
  memoryScope: {
    userId: "local-user",
    profileId: "skyrim-special-edition",
    gameId: "skyrim-special-edition",
    characterId: "lydia",
    sessionId: "response-console-simulation",
    saveId: null,
    crossGameWideningAllowed: false,
  },
};

describe("native product workspaces", () => {
  beforeEach(() => {
    for (const fn of Object.values(bridge)) fn.mockReset();
    bridge.readTarget.mockResolvedValue(null);
    bridge.actorStart.mockResolvedValue({
      schemaVersion: 1,
      state: "unavailable",
      unavailableReason: "noAdmittedNativeCandidateSet",
      detail: "No current admitted native visual candidate set is available.",
    });
    bridge.actorStatus.mockResolvedValue({
      schemaVersion: 1,
      state: "waiting",
      detail: "Waiting for one native click.",
    });
    bridge.actorCancel.mockResolvedValue({
      schemaVersion: 1,
      state: "cancelled",
      detail: "Native actor selection was cancelled.",
    });
    bridge.catalog.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "skyrim-special-edition",
      gameDisplayName: "Skyrim Special Edition",
      selectedCharacterId: null,
      defaultCharacterId: "lydia",
      characters: [
        {
          id: "lydia",
          displayName: "Lydia",
          aliases: [],
          backgroundNpc: false,
          voiceDescription: "Nord",
          identityStrategy: "explicit",
        },
      ],
      editableAuthoredData: true,
    });
    bridge.inspect.mockResolvedValue(inspection);
    bridge.settings.mockResolvedValue(settings);
    bridge.telemetry.mockResolvedValue(telemetry);
    bridge.pack.mockResolvedValue(pack);
    bridge.loadoutPlanner.mockResolvedValue(unavailableLoadoutPlanner);
    bridge.trustedCatalog.mockResolvedValue(trustedCatalog);
    bridge.optionalLifecycle.mockResolvedValue(optionalLifecycle);
    bridge.mutateOptional.mockResolvedValue(optionalLifecycle);
    bridge.cancelOptional.mockResolvedValue(true);
    bridge.activateOptional.mockResolvedValue({
      schemaVersion: 1,
      receipt: {
        schemaVersion: 1,
        identity: { pack_id: "local.llm.qwen", revision: "r1" },
        manifestSha256: "a".repeat(64),
        installedContentTreeSha256: "b".repeat(64),
        attestationSha256: "c".repeat(64),
        providerLoadDurationMillis: 184,
        trustDomain: "local_review_dev_only",
        detail: "The exact packaged provider loaded in a fresh worker.",
      },
      lifecycle: optionalLifecycle,
    });
    bridge.saveSettings.mockImplementation(async (value) => value);
    bridge.selectCharacter.mockResolvedValue({
      gameProfileId: "skyrim-special-edition",
      characterId: "lydia",
      persisted: true,
    });
    bridge.memoryStatus.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "skyrim-special-edition",
      characterId: "lydia",
      deliveredTurns: 1,
      structuredMemories: 2,
      legacyItems: 0,
      hasMemory: true,
      storeSchemaVersion: 3,
      integrityOk: true,
      integrityMessages: ["quick_check ok"],
      databaseBytes: 8192,
      crossGameWideningAllowed: false,
    });
    bridge.listBackups.mockResolvedValue([
      {
        backupId: "backup-existing",
        bytes: 4096,
        containsAllLocalMemory: true,
        localOnly: true,
      },
    ]);
    bridge.backupMemory.mockResolvedValue({
      backupId: "backup-new",
      createdAtMs: 42,
      bytes: 8192,
      pagesCopied: 2,
      containsAllLocalMemory: true,
      localOnly: true,
    });
    bridge.deleteBackup.mockResolvedValue({
      backupId: "backup-existing",
      deleted: true,
    });
    bridge.eraseMemory.mockResolvedValue({
      gameProfileId: "skyrim-special-edition",
      characterId: "lydia",
      erasureId: "erase-1",
      scopeSha256: "c".repeat(64),
      deliveredTurnsDeleted: 1,
      structuredMemoriesDeleted: 2,
      legacyItemsDeleted: 0,
      outboxJobsDeleted: 1,
      erasedAtMs: 43,
      backup: {
        backupId: "backup-before-erase",
        createdAtMs: 42,
        bytes: 8192,
        pagesCopied: 2,
        containsAllLocalMemory: true,
        localOnly: true,
      },
      backupRetainsErasedData: true,
      crossGameWideningAllowed: false,
    });
    bridge.restoreBackup.mockResolvedValue({
      operationId: "restore-1",
      backupId: "backup-existing",
      restored: true,
      priorStoreQuarantined: true,
      reopenedIntegrityOk: true,
      storeSchemaVersion: 3,
      deliveredTurns: 1,
      structuredMemories: 2,
      backupsPreserved: true,
      runtimeRestartsOnNextUse: true,
      auditPersisted: true,
    });
    bridge.removeAllMemory.mockResolvedValue({
      operationId: "remove-all-1",
      removedArtifacts: 3,
      removedBytes: 12_288,
      backupsRemoved: false,
      backupsPreserved: true,
      priorStoreQuarantinesRemoved: 1,
      emptyStoreReopened: true,
      reopenedIntegrityOk: true,
      storeSchemaVersion: 3,
      runtimeRestartsOnNextUse: true,
      auditPersisted: true,
    });
    bridge.correctEncounter.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "skyrim-special-edition",
      event: {
        kind: "corrected_to_authored_character",
        encounter_id: "11111111-1111-4111-8111-111111111111",
        character_id: "lydia",
        source: "manual_explicit",
        occurred_at_ms: 1_700_000_000_000,
        memory_migration_required: true,
      },
      memoryMigrationRequired: true,
      memoryMigrationPerformed: false,
      detail: "Lifecycle updated; memory was not migrated.",
    });
    bridge.mergeEncounters.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "skyrim-special-edition",
      event: {
        kind: "merged_into_encounter",
        source_encounter_id: "11111111-1111-4111-8111-111111111111",
        destination_encounter_id: "22222222-2222-4222-8222-222222222222",
        source: "manual_explicit",
        occurred_at_ms: 1_700_000_000_000,
        memory_migration_required: true,
      },
      memoryMigrationRequired: true,
      memoryMigrationPerformed: false,
      detail: "Lifecycle updated; memory was not migrated.",
    });
    bridge.benchmarkStatus.mockResolvedValue(idleBenchmark);
    bridge.mouthPackState.mockResolvedValue({
      schemaVersion: 1,
      installed: [],
      enabled: [],
      detail: "No character mouth packs are installed.",
    });
  });

  it("keeps browser preview immutable and hardware state explicitly unavailable", () => {
    render(
      <>
        <GameTargetWorkspace
          nativeAvailable={false}
          gameProfiles={[]}
          gameProfileId="eclipse-harbor"
          onGameProfileChange={vi.fn()}
          onSelectionChange={vi.fn()}
        />
        <CharacterDatabase />
        <LocalResourcePlanner models={[]} />
      </>,
    );
    expect(
      screen.getByRole("heading", { name: "Cyberpunk 2077" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Misty Olszewski" }),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent(/Eclipse Harbor|Mara Venn/);
    expect(
      screen.queryByRole("combobox", { name: /game/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(/Character choices are saved by the Windows app/i),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /editable copy|save local/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(/Browser preview has no hardware telemetry/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Native pack lifecycle unavailable/i),
    ).toBeInTheDocument();
  });

  it("loads the native catalog, inspects a character, and persists only selection", async () => {
    const user = userEvent.setup();
    render(
      <CharacterDatabase
        nativeAvailable
        gameProfileId="skyrim-special-edition"
      />,
    );
    expect(
      await screen.findByRole("heading", { name: "Lydia" }),
    ).toBeInTheDocument();
    expect(bridge.catalog).toHaveBeenCalledWith("skyrim-special-edition");
    expect(bridge.inspect).toHaveBeenCalledWith(
      "skyrim-special-edition",
      undefined,
    );
    expect(
      (await screen.findAllByText("Voice only · no mouth pack")).length,
    ).toBeGreaterThan(0);
    expect(screen.getByText("Voice direction included")).toBeVisible();
    expect(screen.getByText("Voice only · mouth pack needed")).toBeVisible();
    await user.click(screen.getByText("Advanced character data"));
    await user.click(
      screen.getByText("Delivered memory · 1", { selector: "summary" }),
    );
    expect(screen.getByText(/Source runtime_turn/i)).toHaveTextContent(
      /request-turn-42/,
    );
    expect(screen.getByText(/Stored record/i)).toHaveTextContent(
      /stored-turn-hash-9f2c/,
    );
    await user.click(
      screen.getByRole("button", { name: "Use this character" }),
    );
    expect(bridge.selectCharacter).toHaveBeenCalledWith(
      "skyrim-special-edition",
      "lydia",
    );
    expect(await screen.findByRole("status")).toHaveTextContent(
      /Selected lydia for ordinary turns/i,
    );
  });

  it("renders a fresh character scope as empty while keeping non-destructive lifecycle actions reachable", async () => {
    const user = userEvent.setup();
    bridge.inspect.mockResolvedValueOnce({
      ...inspection,
      deliveredMemory: [],
    });
    bridge.memoryStatus.mockResolvedValueOnce({
      schemaVersion: 1,
      gameProfileId: "skyrim-special-edition",
      characterId: "lydia",
      deliveredTurns: 0,
      structuredMemories: 0,
      legacyItems: 0,
      hasMemory: false,
      storeSchemaVersion: 3,
      integrityOk: true,
      integrityMessages: ["quick_check ok"],
      databaseBytes: 4096,
      crossGameWideningAllowed: false,
    });
    bridge.listBackups.mockResolvedValueOnce([]);
    render(
      <CharacterDatabase
        nativeAvailable
        gameProfileId="skyrim-special-edition"
      />,
    );

    await screen.findByText("Delivered memory · 0", { selector: "summary" });
    await user.click(screen.getByText("Advanced character data"));
    await user.click(
      screen.getByText("Delivered memory · 0", { selector: "summary" }),
    );
    expect(
      screen.getByText("No delivered turns in the native memory scope."),
    ).toBeVisible();
    expect(screen.getByText("No native local backups reported.")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Back up all local memory" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Erase this character’s memory" }),
    ).toBeDisabled();
    expect(document.body).not.toHaveTextContent(/request-turn|stored-turn/i);
  });

  it("keeps returning-user memory backup and erase controls in the character record with exact receipts", async () => {
    const user = userEvent.setup();
    render(
      <CharacterDatabase
        nativeAvailable
        gameProfileId="skyrim-special-edition"
      />,
    );
    expect(
      await screen.findByRole("heading", {
        name: "Back up or delete memory",
      }),
    ).toBeInTheDocument();
    await user.click(screen.getByText("Advanced character data"));
    expect(
      screen.getByRole("heading", { name: "Back up or delete memory" }),
    ).toBeVisible();
    expect(bridge.memoryStatus).toHaveBeenCalledWith(
      "skyrim-special-edition",
      "lydia",
    );
    expect(bridge.listBackups).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByRole("button", { name: "Back up all local memory" }),
    ).toBeVisible();

    await user.click(
      screen.getByRole("button", { name: "Back up all local memory" }),
    );
    expect(bridge.backupMemory).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", { name: "Confirm whole-store backup" }),
    );
    expect(bridge.backupMemory).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Restore backup" }));
    expect(bridge.restoreBackup).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", { name: "Confirm replace live memory" }),
    );
    expect(bridge.restoreBackup).toHaveBeenCalledWith("backup-existing");
    expect(
      await screen.findByText(/restored backup remains on disk/i),
    ).toBeVisible();

    await user.click(
      screen.getByRole("button", { name: "Remove live local memory" }),
    );
    expect(bridge.removeAllMemory).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", {
        name: "Confirm removal while preserving backups",
      }),
    );
    expect(bridge.removeAllMemory).toHaveBeenCalledWith(false);
    expect(
      await screen.findByText(/Local backups were preserved/i),
    ).toBeVisible();

    bridge.removeAllMemory.mockResolvedValueOnce({
      operationId: "remove-all-2",
      removedArtifacts: 4,
      removedBytes: 16_384,
      backupsRemoved: true,
      backupsPreserved: false,
      priorStoreQuarantinesRemoved: 0,
      emptyStoreReopened: true,
      reopenedIntegrityOk: true,
      storeSchemaVersion: 3,
      runtimeRestartsOnNextUse: true,
      auditPersisted: true,
    });
    await user.click(
      screen.getByRole("checkbox", {
        name: "Also permanently remove every local memory backup",
      }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "Remove all local memory and backups",
      }),
    );
    expect(bridge.removeAllMemory).toHaveBeenCalledTimes(1);
    await user.click(
      screen.getByRole("button", {
        name: "Confirm complete removal including backups",
      }),
    );
    expect(bridge.removeAllMemory).toHaveBeenLastCalledWith(true);
    expect(
      await screen.findByText(/All local backups were removed/i),
    ).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Delete backup" }));
    expect(bridge.deleteBackup).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", { name: "Confirm permanent deletion" }),
    );
    expect(bridge.deleteBackup).toHaveBeenCalledWith("backup-existing");

    await user.click(
      screen.getByRole("button", { name: "Erase this character’s memory" }),
    );
    expect(bridge.eraseMemory).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", {
        name: "Confirm irreversible character erasure",
      }),
    );
    expect(bridge.eraseMemory).toHaveBeenCalledWith(
      "skyrim-special-edition",
      "lydia",
      true,
    );
    expect(
      await screen.findByText(/pre-erasure backup still contains the history/i),
    ).toBeVisible();
    expect(
      screen.getByText(
        /1 delivered turns · 2 structured · 0 legacy · 1 outbox jobs/i,
      ),
    ).toBeVisible();
    expect(document.body).not.toHaveTextContent(
      /memory\.sqlite|\\Users|\/Users/,
    );
  });

  it("discovers and binds an exact current-session game target without capture claims", async () => {
    const user = userEvent.setup();
    const candidate = {
      processId: 44,
      nativeWindow: 55,
      executableName: "Cyberpunk2077.exe",
      executablePathSha256: "a".repeat(64),
      title: "Cyberpunk 2077",
      foreground: true,
      clientWidth: 1920,
      clientHeight: 1080,
    };
    bridge.discover.mockResolvedValue([candidate]);
    bridge.selectTarget.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "cyberpunk-2077",
      target: candidate,
      processInstanceBound: true,
      userConfirmedOfflineSinglePlayer: true,
      captureAuthorized: false,
      safetyState: "unverified",
      safetyDetail: "Visual capture stays blocked.",
    });
    render(
      <GameTargetWorkspace
        nativeAvailable
        gameProfiles={[
          {
            id: "cyberpunk-2077",
            displayName: "Cyberpunk 2077",
            wave: "1",
            safety: "singlePlayerOnly",
            catalogState: "bundled",
            defaultFallback: "audioOnly",
          },
          {
            id: "skyrim-special-edition",
            displayName: "Skyrim Special Edition",
            wave: "1",
            safety: "singlePlayerOnly",
            catalogState: "bundled",
            defaultFallback: "audioOnly",
          },
        ]}
        gameProfileId="skyrim-special-edition"
        onGameProfileChange={vi.fn()}
        onSelectionChange={vi.fn()}
      />,
    );
    await screen.findByText(
      /No game process is selected in this native session/i,
    );
    await user.click(
      screen.getByRole("button", { name: "Scan for Cyberpunk 2077" }),
    );
    await user.click(
      screen.getByRole("checkbox", { name: /single-player session/i }),
    );
    await user.click(
      screen.getByRole("button", { name: "Connect this window" }),
    );
    expect(bridge.discover).toHaveBeenCalledWith("cyberpunk-2077");
    expect(bridge.selectTarget).toHaveBeenCalledWith(
      "cyberpunk-2077",
      55,
      true,
    );
    expect(
      (await screen.findAllByText("Visual capture stays blocked.")).length,
    ).toBeGreaterThan(0);
    await user.click(screen.getByText("Connection details"));
    expect(
      screen.getByRole("button", { name: "Check game capture" }),
    ).toBeEnabled();
    bridge.verifyCapture.mockRejectedValueOnce(
      new Error("No current game frame available."),
    );
    await user.click(
      screen.getByRole("button", { name: "Check game capture" }),
    );
    expect(bridge.verifyCapture).toHaveBeenCalled();
    expect(
      await screen.findByText("No current game frame available."),
    ).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "Select NPC on screen" }),
    );
    expect(bridge.actorStart).toHaveBeenCalledWith();
    expect(
      await screen.findByText(
        /No current admitted native visual candidate set/i,
      ),
    ).toBeInTheDocument();
    expect(
      within(
        screen.getByRole("region", { name: "NPC tracking setup" }),
      ).getByText("Not selected"),
    ).toBeVisible();
    expect(
      screen.queryByText("Skyrim Special Edition"),
    ).not.toBeInTheDocument();
  });

  it("restores the single-player confirmation from an existing native target", async () => {
    const candidate = {
      processId: 44,
      nativeWindow: 55,
      executableName: "Cyberpunk2077.exe",
      executablePathSha256: "a".repeat(64),
      title: "Cyberpunk 2077",
      foreground: true,
      clientWidth: 1920,
      clientHeight: 1080,
    };
    bridge.readTarget.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "cyberpunk-2077",
      target: candidate,
      processInstanceBound: true,
      userConfirmedOfflineSinglePlayer: true,
      captureAuthorized: true,
      safetyState: "allowed",
      safetyDetail: "Connected native game target.",
    });

    render(
      <GameTargetWorkspace
        nativeAvailable
        gameProfiles={[
          {
            id: "cyberpunk-2077",
            displayName: "Cyberpunk 2077",
            wave: "1",
            safety: "singlePlayerOnly",
            catalogState: "bundled",
            defaultFallback: "audioOnly",
          },
        ]}
        gameProfileId="cyberpunk-2077"
        onGameProfileChange={vi.fn()}
        onSelectionChange={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("checkbox", { name: /single-player session/i }),
    ).toBeChecked();
  });

  it("requires explicit confirmation for native encounter correction and merge receipts", async () => {
    const user = userEvent.setup();
    render(
      <EncounterLifecycleControls
        nativeAvailable
        encounter={{
          schema_version: "character-db/1.0.0",
          encounter_id: "11111111-1111-4111-8111-111111111111",
          game_profile_id: "skyrim-special-edition",
          archetype_character_id: "background-npc",
          continuity_key_sha256: "c".repeat(64),
          selected_voice: {
            binding_id: "voice-binding",
            adapter_id: "adapter",
            provider_voice_id: "stock-voice",
            locale: "en-US",
            traits: [],
          },
          created_at_ms: 1,
          last_seen_at_ms: 2,
          expires_at_ms: 10_000,
          status: "active",
        }}
        authoredCharacter={{
          gameProfileId: "skyrim-special-edition",
          characterId: "lydia",
          displayName: "Lydia",
        }}
      />,
    );
    const correct = screen.getByRole("button", {
      name: "Correct encounter to authored character",
    });
    const merge = screen.getByRole("button", {
      name: "Merge into destination",
    });
    expect(correct).toBeDisabled();
    expect(merge).toBeDisabled();
    await user.click(
      screen.getByRole("checkbox", { name: /explicitly confirm/i }),
    );
    expect(correct).toBeEnabled();
    await user.click(correct);
    expect(bridge.correctEncounter).toHaveBeenCalledWith({
      gameProfileId: "skyrim-special-edition",
      encounterId: "11111111-1111-4111-8111-111111111111",
      characterId: "lydia",
      explicitUserConfirmation: true,
    });
    expect(await screen.findByRole("status")).toHaveTextContent(
      /Memory migration required: yes · performed: no/i,
    );

    await user.type(
      screen.getByLabelText("Destination encounter ID"),
      "22222222-2222-4222-8222-222222222222",
    );
    await user.click(merge);
    expect(bridge.mergeEncounters).toHaveBeenCalledWith({
      gameProfileId: "skyrim-special-edition",
      sourceEncounterId: "11111111-1111-4111-8111-111111111111",
      destinationEncounterId: "22222222-2222-4222-8222-222222222222",
      explicitUserConfirmation: true,
    });
    expect(await screen.findByRole("status")).toHaveTextContent(
      "merged_into_encounter",
    );
  });

  it("persists native reserves and keeps activation disabled on incomplete telemetry", async () => {
    const user = userEvent.setup();
    render(<LocalResourcePlanner models={[]} nativeAvailable />);
    expect(await screen.findByText("Test GPU")).toBeInTheDocument();
    expect(screen.getByText(/12.0 GiB/)).toBeInTheDocument();
    await user.clear(screen.getByLabelText("Game VRAM reserve in GiB"));
    await user.type(screen.getByLabelText("Game VRAM reserve in GiB"), "6");
    await user.clear(screen.getByLabelText("VRAM soft ceiling percent"));
    await user.type(screen.getByLabelText("VRAM soft ceiling percent"), "80");
    await user.clear(
      screen.getByLabelText("Keep local models warm in seconds"),
    );
    await user.type(
      screen.getByLabelText("Keep local models warm in seconds"),
      "45",
    );
    await user.click(
      screen.getByRole("button", { name: "Save native resource policy" }),
    );
    await waitFor(() =>
      expect(bridge.saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          gameReserveVramBytes: 6 * 1024 ** 3,
          governor: expect.objectContaining({
            vram_soft_ceiling_basis_points: 8000,
            keep_warm_millis: 45000,
          }),
        }),
      ),
    );
    await user.click(
      screen.getByRole("checkbox", { name: /explicit local-review mutation/i }),
    );
    expect(
      screen.getByRole("button", { name: "Request activation check" }),
    ).toBeDisabled();
    expect(
      screen.getAllByText(
        /activation is fail-closed because telemetry is incomplete/i,
      ).length,
    ).toBeGreaterThan(0);
  });

  it("renders only signed pack runtime, license, lifecycle, and measurement truth", async () => {
    render(<LocalResourcePlanner models={[]} nativeAvailable />);

    expect(
      await screen.findByRole("heading", { name: "Qwen local conversation" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/llama\.cpp b7000 · cpu \/ vulkan \/ cuda/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Apache-2.0" })).toHaveAttribute(
      "href",
      "https://www.apache.org/licenses/LICENSE-2.0",
    );
    expect(screen.getByText("Not measured")).toBeInTheDocument();
    expect(
      screen.getByText(/Non-production review trust/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Review evidence is not admission evidence/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/non-qualifying review evidence 2/i),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("LM Studio");
  });

  it("requires explicit signed-catalog consent before an optional pack mutation", async () => {
    const user = userEvent.setup();
    render(<LocalResourcePlanner models={[]} nativeAvailable />);

    await screen.findByRole("heading", { name: "Qwen local conversation" });
    const install = screen.getByRole("button", {
      name: "Install exact optional pack",
    });
    expect(install).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "Repair from signed catalog" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Remove exact pack" }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("checkbox", {
        name: /allow changes to this local model/i,
      }),
    );
    expect(install).toBeEnabled();
    await user.click(install);

    await waitFor(() =>
      expect(bridge.mutateOptional).toHaveBeenCalledWith(
        "install",
        { pack_id: "local.llm.qwen", revision: "r1" },
        false,
      ),
    );
  });

  it("cancels only the exact active optional-pack transfer and renders its receipt", async () => {
    const user = userEvent.setup();
    let finishInstall: ((value: typeof optionalLifecycle) => void) | undefined;
    bridge.mutateOptional.mockImplementationOnce(
      () =>
        new Promise<typeof optionalLifecycle>((resolve) => {
          finishInstall = resolve;
        }),
    );
    render(<LocalResourcePlanner models={[]} nativeAvailable />);
    await screen.findByRole("heading", { name: "Qwen local conversation" });
    await user.click(
      screen.getByRole("checkbox", {
        name: /allow changes to this local model/i,
      }),
    );
    await user.click(
      screen.getByRole("button", { name: "Install exact optional pack" }),
    );
    await user.click(
      await screen.findByRole("button", {
        name: "Cancel active exact-pack transfer",
      }),
    );
    expect(bridge.cancelOptional).toHaveBeenCalledWith({
      pack_id: "local.llm.qwen",
      revision: "r1",
    });
    expect(
      await screen.findByText(/cancellation requested.*local\.llm\.qwen@r1/i),
    ).toHaveAttribute("role", "status");
    finishInstall?.(optionalLifecycle);
    await waitFor(() => expect(bridge.mutateOptional).toHaveBeenCalledTimes(1));
  });

  it("activates a selected installed pack through a setup-only provider-load self-test", async () => {
    const user = userEvent.setup();
    const admittedSelection = {
      ...selectedLoadout,
      roles: [
        {
          role: "language_model" as const,
          identity: { pack_id: "local.llm.qwen", revision: "r1" },
          preferred_residency: "cpu_resident_gpu_cold" as const,
        },
      ],
    };
    const awaitingLifecycle = {
      ...optionalLifecycle,
      packs: [
        {
          ...optionalLifecycle.packs[0],
          phase: "installed_inactive_awaiting_self_test" as const,
          canInstall: false,
          canRemove: true,
          detail: "Installed and awaiting an attested provider-load self-test.",
        },
      ],
    };
    const activeLifecycle = {
      ...awaitingLifecycle,
      packs: [
        {
          ...awaitingLifecycle.packs[0],
          phase: "active" as const,
          detail: "Active after the attested provider-load self-test.",
        },
      ],
    };
    bridge.loadoutPlanner.mockResolvedValue({
      schemaVersion: 1,
      ready: true,
      detail: "Exact local selection is ready for a measured check.",
      selected: admittedSelection,
      planner: {
        schema: "npc.selected-loadout-admission/v1",
        trusted_measurement_streams: 1,
        active_evidence: [admittedSelection.roles[0].identity],
        pending_work: 0,
        pending_by_kind: {},
      },
    });
    bridge.optionalLifecycle.mockResolvedValue(awaitingLifecycle);
    bridge.activateOptional.mockResolvedValue({
      schemaVersion: 1,
      receipt: {
        schemaVersion: 1,
        identity: admittedSelection.roles[0].identity,
        manifestSha256: "a".repeat(64),
        installedContentTreeSha256: "b".repeat(64),
        attestationSha256: "c".repeat(64),
        providerLoadDurationMillis: 184,
        trustDomain: "local_review_dev_only",
        detail: "The exact packaged provider loaded in a fresh worker.",
      },
      lifecycle: activeLifecycle,
    });

    render(<LocalResourcePlanner models={[]} nativeAvailable />);
    await screen.findByRole("heading", { name: "Qwen local conversation" });
    const activate = screen.getByRole("button", {
      name: "Test & activate",
    });
    expect(activate).toBeEnabled();
    expect(
      screen.getByText(/no game needs to be running/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/loads and unloads the model on this PC/i),
    ).toBeInTheDocument();
    await user.click(activate);

    await waitFor(() =>
      expect(bridge.activateOptional).toHaveBeenCalledWith(
        admittedSelection.roles[0].identity,
        admittedSelection,
      ),
    );
    expect(await screen.findByText("Install check passed")).toBeInTheDocument();
    expect(screen.getByText("184 ms")).toBeInTheDocument();
    expect(
      screen.getByText(/animation quality is not rated by this check/i),
    ).toBeInTheDocument();
  });

  it("refreshes the lifecycle after a provider-load activation error", async () => {
    const user = userEvent.setup();
    const admittedSelection = {
      ...selectedLoadout,
      roles: [
        {
          role: "language_model" as const,
          identity: { pack_id: "local.llm.qwen", revision: "r1" },
          preferred_residency: "cpu_resident_gpu_cold" as const,
        },
      ],
    };
    const awaitingLifecycle = {
      ...optionalLifecycle,
      packs: [
        {
          ...optionalLifecycle.packs[0],
          phase: "installed_inactive_awaiting_self_test" as const,
          canInstall: false,
          canRemove: true,
        },
      ],
    };
    const activeLifecycle = {
      ...awaitingLifecycle,
      packs: [
        {
          ...awaitingLifecycle.packs[0],
          phase: "active" as const,
          detail: "The model is already active.",
        },
      ],
    };
    bridge.loadoutPlanner.mockResolvedValue({
      schemaVersion: 1,
      ready: true,
      detail: "Exact local selection is ready for a measured check.",
      selected: admittedSelection,
      planner: null,
    });
    bridge.optionalLifecycle
      .mockResolvedValueOnce(awaitingLifecycle)
      .mockResolvedValueOnce(activeLifecycle);
    bridge.activateOptional.mockRejectedValue(
      new Error("The model became active before the receipt returned."),
    );

    render(<LocalResourcePlanner models={[]} nativeAvailable />);
    await user.click(
      await screen.findByRole("button", { name: "Test & activate" }),
    );

    expect(
      await screen.findByText(/became active before the receipt returned/i),
    ).toHaveAttribute("role", "alert");
    expect(bridge.optionalLifecycle).toHaveBeenCalledTimes(2);
    expect(screen.getByText("Installed and active.")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Test & activate" }),
    ).not.toBeInTheDocument();
  });

  it("keeps provider-load activation blocked when the selected draft duplicates the exact pack", async () => {
    const exactRole = {
      role: "language_model" as const,
      identity: { pack_id: "local.llm.qwen", revision: "r1" },
      preferred_residency: "cpu_resident_gpu_cold" as const,
    };
    bridge.loadoutPlanner.mockResolvedValue({
      schemaVersion: 1,
      ready: true,
      detail: "The selected draft contains a duplicate exact pack.",
      selected: {
        ...selectedLoadout,
        roles: [exactRole, { ...exactRole, role: "other" as const }],
      },
      planner: null,
    });
    bridge.optionalLifecycle.mockResolvedValue({
      ...optionalLifecycle,
      packs: [
        {
          ...optionalLifecycle.packs[0],
          phase: "installed_inactive_awaiting_self_test" as const,
          canInstall: false,
          canRemove: true,
        },
      ],
    });

    render(<LocalResourcePlanner models={[]} nativeAvailable />);
    await screen.findByRole("heading", { name: "Qwen local conversation" });

    expect(
      screen.getByRole("button", {
        name: "Test & activate",
      }),
    ).toBeDisabled();
    expect(screen.getByText(/select this model once/i)).toBeInTheDocument();
    expect(bridge.activateOptional).not.toHaveBeenCalled();
  });

  it("passes only the exact native-selected snake-case loadout into admission", async () => {
    const user = userEvent.setup();
    bridge.loadoutPlanner.mockResolvedValue({
      schemaVersion: 1,
      ready: true,
      detail: "Release-threshold planner is ready.",
      selected: selectedLoadout,
      planner: {
        schema: "npc.selected-loadout-admission/v1",
        trusted_measurement_streams: 1,
        active_evidence: [],
        pending_work: 0,
        pending_by_kind: {},
      },
    });
    bridge.admitLoadout.mockResolvedValue({
      schemaVersion: 1,
      ready: true,
      detail: "No signed current-device measurement exists.",
      persisted: false,
      decision: {
        schema: "npc.selected-loadout-admission/v1",
        selection_id: selectedLoadout.selection_id,
        status: "blocked",
        reason_code: "missing_measured_envelope",
        detail: "No signed current-device measurement exists.",
        exact_target_pid: 44,
        selected_roles: selectedLoadout.roles,
        live_snapshot: null,
        admission_receipt: null,
        residency_decisions: [],
        pressure_cancellations: [],
      },
    });
    render(<LocalResourcePlanner models={[]} nativeAvailable />);
    expect(
      await screen.findByText("selected-local-loadout-1"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: "Check and admit selected loadout",
      }),
    );
    expect(bridge.admitLoadout).toHaveBeenCalledWith(selectedLoadout);
    expect(
      await screen.findByText("missing_measured_envelope"),
    ).toBeInTheDocument();
    expect(screen.getByText("blocked")).toBeInTheDocument();
  });

  it("starts and cancels only the bounded native benchmark request", async () => {
    const user = userEvent.setup();
    bridge.benchmarkStart.mockResolvedValue({
      ...idleBenchmark,
      state: "running",
      requestedIterations: 10,
      startedAtUtc: "2026-08-30T10:00:00Z",
    });
    bridge.benchmarkCancel.mockResolvedValue({
      ...idleBenchmark,
      state: "cancelling",
      requestedIterations: 10,
      completedIterations: 2,
      startedAtUtc: "2026-08-30T10:00:00Z",
      elapsedMillis: 2000,
    });
    render(<ThisPcBenchmark nativeAvailable />);
    await screen.findByText("Idle");
    await user.click(
      screen.getByRole("button", { name: "Start native benchmark" }),
    );
    expect(bridge.benchmarkStart).toHaveBeenCalledWith({
      requestedIterations: 10,
      timeoutMillis: 120000,
      baselineWindowMillis: 3000,
    });
    expect(await screen.findByText("Running")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel run" }));
    expect(bridge.benchmarkCancel).toHaveBeenCalledWith();
    expect(await screen.findByText("Cancelling")).toBeInTheDocument();
  });

  it("loads a terminal native report without filling absent frame measurements with zero", async () => {
    bridge.benchmarkStatus.mockResolvedValue({
      ...idleBenchmark,
      state: "partial",
      reportId: "report-1",
      requestedIterations: 10,
      completedIterations: 4,
      startedAtUtc: "2026-08-30T10:00:00Z",
      elapsedMillis: 42000,
      reportFileName: "this-pc-benchmark-report-1.json",
      unavailableComponents: ["game_frame_sampler"],
      actionCodes: ["review_diagnostics"],
    });
    bridge.benchmarkReport.mockResolvedValue(benchmarkReport);
    render(<ThisPcBenchmark nativeAvailable />);
    expect(
      await screen.findByText("Llm Time To First Token Ms"),
    ).toBeInTheDocument();
    expect(bridge.benchmarkReport).toHaveBeenCalledWith("report-1");
    expect(screen.getByText("120.0 ms")).toBeInTheDocument();
    expect(screen.getAllByText("Unavailable").length).toBeGreaterThan(0);
    expect(screen.getByText("Not review-eligible")).toBeInTheDocument();
    expect(screen.queryByText("0.0 ms")).not.toBeInTheDocument();
  });
});
