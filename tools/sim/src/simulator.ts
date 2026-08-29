// @ts-check

const { canonicalJsonLines, sha256 } = require("./canonical.ts");
const { VirtualClock, SeededRandom } = require("./virtual-clock.ts");

const EVENT_VERSION = "npc.sim.event.v1";
const SPINE = ["listening", "transcribing", "identifying", "remembering", "responding", "voicing", "animating"];

/** @param {any} scenario @param {any} resourceProfile */
function simulate(scenario, resourceProfile) {
  const clock = new VirtualClock();
  const random = new SeededRandom(scenario.seed);
  const generation = { value: 0 };
  /** @type {any[]} */
  const events = [];
  /** @type {Map<string, string>} */
  const stageStatus = new Map(SPINE.map((stage) => [stage, "pending"]));
  /** @type {Set<string>} */
  const degradationReasons = new Set();
  /** @type {Set<string>} */
  const quarantinedFeatures = new Set();
  let sequence = 0;
  let terminalStatus = "running";
  let speechEndMs = null;
  let firstAudioMs = null;
  let networkAttempts = 0;
  let animationDisabled = scenario.turn.npc.visibility !== "visible";
  let animationExpected = scenario.stream.some((entry) => entry.source === "animation");

  /** @param {string} kind @param {string} source @param {Record<string, unknown>} payload @param {string | null} [stage] */
  function emit(kind, source, payload, stage = null) {
    events.push({
      schemaVersion: EVENT_VERSION,
      scenarioId: scenario.id,
      sequence: sequence++,
      virtualTimeMs: clock.nowMs,
      sessionId: scenario.turn.sessionId,
      turnId: scenario.turn.turnId,
      cancellationGeneration: generation.value,
      kind,
      source,
      stage,
      payload,
    });
  }

  /** @param {string} stage @param {string} status @param {Record<string, unknown>} [details] */
  function transition(stage, status, details = {}) {
    const previous = stageStatus.get(stage);
    if (previous === status) return;
    stageStatus.set(stage, status);
    emit("response_spine.stage", "runtime", { previous, status, ...details }, stage);
  }

  /** @param {string} reason @param {Record<string, unknown>} [details] */
  function degrade(reason, details = {}) {
    if (degradationReasons.has(reason)) return;
    degradationReasons.add(reason);
    emit("runtime.degraded", "resource_broker", { reason, ...details });
  }

  /** @param {string} source @param {string} kind @param {Record<string, unknown>} payload */
  function handleStream(source, kind, payload) {
    if (terminalStatus === "cancelled" || terminalStatus === "failed") {
      emit("late_event.ignored", source, { kind, reason: terminalStatus, originalPayload: payload });
      return;
    }

    if (quarantinedFeatures.has(source)) {
      emit("quarantined_event.ignored", source, { kind, reason: "worker_quarantined" });
      return;
    }

    if (source === "control" && kind === "barge_in") {
      generation.value += 1;
      terminalStatus = "cancelled";
      emit("turn.cancelled", "control", { reason: payload.reason || "barge_in", cancelledGeneration: generation.value - 1 });
      for (const stage of SPINE) {
        if (stageStatus.get(stage) === "active") transition(stage, "cancelled", { reason: "barge_in" });
      }
      return;
    }

    if (source === "stt" && kind === "partial") {
      emit("transcript.partial", "stt", payload, "transcribing");
      transition("transcribing", "active");
      return;
    }

    if (source === "stt" && kind === "final") {
      speechEndMs = clock.nowMs;
      emit("transcript.final", "stt", payload, "transcribing");
      transition("listening", "completed");
      transition("transcribing", "completed");
      transition("identifying", "active");
      return;
    }

    if (source === "identity" && kind === "resolved") {
      emit("identity.resolved", "identity", {
        npcId: scenario.turn.npc.id,
        npcName: scenario.turn.npc.name,
        visibility: scenario.turn.npc.visibility,
        ...payload,
      }, "identifying");
      transition("identifying", "completed");
      transition("remembering", "active");
      if (scenario.turn.npc.visibility !== "visible") {
        animationDisabled = true;
        transition("animating", "skipped", { reason: "npc_not_visible", fallback: "audio_and_subtitles" });
      }
      return;
    }

    if (source === "memory" && kind === "retrieved") {
      emit("memory.retrieved", "memory", payload, "remembering");
      transition("remembering", "completed");
      transition("responding", "active");
      return;
    }

    if (source === "llm" && kind === "token") {
      emit("response.delta", "llm", payload, "responding");
      return;
    }

    if (source === "llm" && kind === "sentence") {
      emit("sentence.ready", "llm", payload, "responding");
      return;
    }

    if (source === "llm" && kind === "complete") {
      emit("response.complete", "llm", payload, "responding");
      transition("responding", "completed");
      return;
    }

    if (source === "tts" && kind === "audio_chunk") {
      if (firstAudioMs === null) firstAudioMs = clock.nowMs;
      if (stageStatus.get("voicing") === "pending") transition("voicing", "active");
      emit("audio.chunk", "tts", payload, "voicing");
      return;
    }

    if (source === "tts" && kind === "complete") {
      emit("audio.complete", "tts", payload, "voicing");
      transition("voicing", "completed");
      if (!animationExpected || animationDisabled) {
        if (stageStatus.get("animating") === "pending") transition("animating", "skipped", { reason: "animation_not_requested" });
      }
      return;
    }

    if (source === "animation" && kind === "viseme") {
      if (animationDisabled) {
        emit("animation.skipped", "animation", { reason: "visuals_disabled", originalPayload: payload }, "animating");
        return;
      }
      if (stageStatus.get("animating") === "pending") transition("animating", "active");
      emit("animation.viseme", "animation", payload, "animating");
      return;
    }

    if (source === "animation" && kind === "complete") {
      if (animationDisabled) {
        emit("animation.skipped", "animation", { reason: "visuals_disabled" }, "animating");
        if (stageStatus.get("animating") !== "skipped") transition("animating", "skipped", { reason: "visuals_disabled" });
        return;
      }
      emit("animation.complete", "animation", payload, "animating");
      transition("animating", "completed");
      return;
    }

    emit("stream.event", source, { kind, ...payload });
  }

  /** @param {any} fault */
  function handleFault(fault) {
    if (terminalStatus === "cancelled" || terminalStatus === "failed") {
      emit("fault.ignored", "fault_injector", { target: fault.target, kind: fault.kind, code: fault.code, reason: terminalStatus });
      return;
    }
    emit("fault.injected", "fault_injector", {
      target: fault.target,
      kind: fault.kind,
      code: fault.code,
      retryable: Boolean(fault.retryable),
    }, fault.target === "resource" ? null : fault.target);

    if (fault.kind === "low_vram") {
      animationDisabled = true;
      degrade("low_vram", { freeVramMiB: fault.freeVramMiB, requiredVramMiB: fault.requiredVramMiB });
      degrade("continuous_vision_disabled");
      degrade("screen_space_lipsync_disabled");
      if (stageStatus.get("animating") === "pending") transition("animating", "skipped", { reason: "low_vram" });
      return;
    }

    if (fault.kind === "worker_crash" && fault.target === "animation") {
      animationDisabled = true;
      quarantinedFeatures.add("animation");
      degrade("animation_worker_quarantined", { code: fault.code, fallback: "audio_and_subtitles" });
      transition("animating", "skipped", { reason: "worker_crash", code: fault.code });
      return;
    }

    if (fault.kind === "worker_crash" && ["identity", "memory"].includes(fault.target)) {
      quarantinedFeatures.add(fault.target);
      degrade(`${fault.target}_worker_quarantined`, { code: fault.code });
      transition(fault.target === "identity" ? "identifying" : "remembering", "skipped", { reason: "worker_crash" });
      return;
    }

    terminalStatus = "failed";
    const stage = fault.target === "stt" ? "transcribing"
      : fault.target === "llm" ? "responding"
      : fault.target === "tts" ? "voicing"
      : fault.target;
    if (SPINE.includes(stage)) transition(stage, "failed", { code: fault.code, retryable: Boolean(fault.retryable) });
    emit("turn.error", "runtime", { source: fault.target, code: fault.code, retryable: Boolean(fault.retryable) }, SPINE.includes(stage) ? stage : null);
  }

  const deterministicRunId = `run-${Math.floor(random.next() * 0xffffffff).toString(16).padStart(8, "0")}`;
  emit("scenario.started", "simulator", {
    deterministicRunId,
    seed: scenario.seed,
    manifestVersion: scenario.schemaVersion,
    description: scenario.description,
  });
  emit("resource.snapshot", "resource_broker", {
    profileId: resourceProfile.id,
    powerMode: resourceProfile.power.mode,
    cpuBoostEnabled: resourceProfile.power.cpuBoostEnabled,
    competingAgents: resourceProfile.workload.competingAgents,
    canonicalBenchmark: resourceProfile.benchmark.canonical,
    vramBudgetMiB: resourceProfile.resources.vramBudgetMiB,
    vramFreeMiB: resourceProfile.resources.vramFreeMiB,
  });
  emit("privacy.policy", "policy", {
    mode: scenario.privacy.mode,
    networkAllowed: scenario.privacy.networkAllowed,
    providers: scenario.providers,
  });
  transition("listening", "active");

  // Faults are queued first so a fault at the same virtual instant prevents a mocked
  // provider event from being observed. This ordering is part of the v1 contract.
  for (const fault of scenario.faults) {
    clock.schedule(fault.atMs, `fault:${fault.target}:${fault.kind}`, () => handleFault(fault));
  }
  for (const [index, entry] of scenario.stream.entries()) {
    const scheduledGeneration = generation.value;
    clock.schedule(entry.atMs, `stream:${index}:${entry.source}:${entry.kind}`, () => {
      if (scheduledGeneration !== generation.value) {
        emit("late_event.ignored", entry.source, { kind: entry.kind, reason: "stale_cancellation_generation", scheduledGeneration });
        return;
      }
      if (entry.payload.networkRequest === true) {
        networkAttempts += 1;
        if (!scenario.privacy.networkAllowed) {
          emit("network.request_blocked", "policy", { provider: entry.payload.provider || "unknown", source: entry.source });
          terminalStatus = "failed";
          return;
        }
      }
      handleStream(entry.source, entry.kind, entry.payload);
    });
  }
  clock.run();

  if (terminalStatus === "running") {
    terminalStatus = degradationReasons.size > 0 ? "degraded" : "completed";
  }
  if (stageStatus.get("listening") === "active" && terminalStatus !== "completed") {
    transition("listening", terminalStatus === "cancelled" ? "cancelled" : "failed", { reason: "turn_terminal" });
  }

  const metrics = {
    speechEndMs,
    firstAudioMs,
    speechEndToFirstAudioMs: speechEndMs !== null && firstAudioMs !== null ? firstAudioMs - speechEndMs : null,
    networkAttempts,
    eventCountBeforeSummary: events.length,
  };
  emit("scenario.finished", "simulator", {
    status: terminalStatus,
    degradationReasons: [...degradationReasons].sort(),
    quarantinedFeatures: [...quarantinedFeatures].sort(),
    metrics,
    spine: Object.fromEntries(SPINE.map((stage) => [stage, stageStatus.get(stage)])),
  });

  const jsonl = canonicalJsonLines(events);
  return {
    events,
    jsonl,
    traceSha256: sha256(jsonl),
    status: terminalStatus,
    degradationReasons: [...degradationReasons].sort(),
    metrics,
    spine: Object.fromEntries(SPINE.map((stage) => [stage, stageStatus.get(stage)])),
  };
}

/** @param {any} scenario @param {ReturnType<typeof simulate>} result */
function assertExpected(scenario, result) {
  const errors = [];
  if (scenario.expected.status !== result.status) {
    errors.push(`status: expected ${scenario.expected.status}, received ${result.status}`);
  }
  if (scenario.expected.traceSha256 && scenario.expected.traceSha256 !== result.traceSha256) {
    errors.push(`traceSha256: expected ${scenario.expected.traceSha256}, received ${result.traceSha256}`);
  }
  if (scenario.expected.firstAudioAtMs !== undefined && scenario.expected.firstAudioAtMs !== result.metrics.firstAudioMs) {
    errors.push(`firstAudioAtMs: expected ${scenario.expected.firstAudioAtMs}, received ${result.metrics.firstAudioMs}`);
  }
  if (scenario.expected.speechEndToFirstAudioMs !== undefined && scenario.expected.speechEndToFirstAudioMs !== result.metrics.speechEndToFirstAudioMs) {
    errors.push(`speechEndToFirstAudioMs: expected ${scenario.expected.speechEndToFirstAudioMs}, received ${result.metrics.speechEndToFirstAudioMs}`);
  }
  if (scenario.expected.networkAttempts !== undefined && scenario.expected.networkAttempts !== result.metrics.networkAttempts) {
    errors.push(`networkAttempts: expected ${scenario.expected.networkAttempts}, received ${result.metrics.networkAttempts}`);
  }
  if (scenario.expected.degradationReasons) {
    const expected = [...scenario.expected.degradationReasons].sort();
    if (JSON.stringify(expected) !== JSON.stringify(result.degradationReasons)) {
      errors.push(`degradationReasons: expected ${JSON.stringify(expected)}, received ${JSON.stringify(result.degradationReasons)}`);
    }
  }
  if (errors.length > 0) throw new Error(`${scenario.id} expectation failure:\n- ${errors.join("\n- ")}`);
}

module.exports = { EVENT_VERSION, SPINE, simulate, assertExpected };
