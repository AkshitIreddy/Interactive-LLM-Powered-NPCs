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
  let inputReadyMs = null;
  let firstAudioMs = null;
  let lastFailureMs = null;
  let lastRecoveryMs = null;
  let networkAttempts = 0;
  let animationDisabled = scenario.turn.npc.visibility !== "visible";
  let animationExpected = scenario.stream.some((entry) => entry.source === "animation");
  let audioCompleted = false;
  let activeTrackId = null;
  let activeTrackEpoch = null;
  let deliveryCommitCount = 0;
  let rejectedDeliveryCommitCount = 0;
  let manualRetryCount = 0;
  let runtimeRestartCount = 0;
  let identityAmbiguityCount = 0;
  let identityMissCount = 0;
  let staleLipSyncDropCount = 0;

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

  /** @param {string} reason */
  function resetTurnAttempt(reason) {
    audioCompleted = false;
    for (const stage of SPINE) stageStatus.set(stage, "pending");
    emit("turn.attempt_reset", "runtime", { reason, generation: generation.value });
  }

  /** @param {string} source @param {string} kind @param {Record<string, unknown>} payload */
  function handleStream(source, kind, payload) {
    if (source === "control" && kind === "manual_retry") {
      if (terminalStatus !== "failed") {
        emit("turn.retry_rejected", "control", { reason: "turn_not_retryable", status: terminalStatus });
        return;
      }
      const previousGeneration = generation.value;
      generation.value += 1;
      manualRetryCount += 1;
      terminalStatus = "running";
      lastRecoveryMs = clock.nowMs;
      resetTurnAttempt("manual_retry");
      emit("turn.retry_started", "control", {
        previousGeneration,
        generation: generation.value,
        reason: payload.reason || "manual_retry",
      });
      return;
    }

    if (source === "control" && kind === "runtime_restart") {
      if (terminalStatus !== "crashed") {
        emit("runtime.restart_rejected", "control", { reason: "runtime_not_crashed", status: terminalStatus });
        return;
      }
      const previousGeneration = generation.value;
      generation.value += 1;
      runtimeRestartCount += 1;
      terminalStatus = "running";
      lastRecoveryMs = clock.nowMs;
      resetTurnAttempt("runtime_restart");
      emit("runtime.restarted", "control", {
        previousGeneration,
        generation: generation.value,
        recoveredFrom: payload.recoveredFrom || "runtime_crash",
      });
      return;
    }

    if (["cancelled", "failed", "crashed"].includes(terminalStatus)) {
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

    if (source === "input" && kind === "typed_input") {
      inputReadyMs = clock.nowMs;
      emit("input.typed", "input", payload);
      transition("listening", "skipped", { reason: "typed_input" });
      transition("transcribing", "skipped", { reason: "typed_input" });
      transition("identifying", "active");
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
      activeTrackId = payload.actorTrackId || scenario.turn.npc.id;
      activeTrackEpoch = payload.trackEpoch ?? 0;
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

    if (source === "identity" && kind === "ambiguous") {
      identityAmbiguityCount += 1;
      emit("identity.ambiguous", "identity", payload, "identifying");
      if (stageStatus.get("identifying") === "pending") transition("identifying", "active");
      return;
    }

    if (source === "identity" && kind === "missed") {
      identityMissCount += 1;
      emit("identity.missed", "identity", payload, "identifying");
      if (stageStatus.get("identifying") === "pending") transition("identifying", "active");
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
      audioCompleted = true;
      emit("audio.complete", "tts", payload, "voicing");
      transition("voicing", "completed");
      if (!animationExpected || animationDisabled) {
        if (stageStatus.get("animating") === "pending") transition("animating", "skipped", { reason: "animation_not_requested" });
      }
      return;
    }

    if (source === "delivery" && kind === "commit") {
      if (!audioCompleted) {
        rejectedDeliveryCommitCount += 1;
        emit("delivery.commit_rejected", "delivery", {
          reason: "audio_not_completed",
          sentenceId: payload.sentenceId || null,
        });
        return;
      }
      deliveryCommitCount += 1;
      emit("delivery.committed", "delivery", {
        sentenceId: payload.sentenceId || null,
        deliveredText: payload.deliveredText || null,
        generation: generation.value,
      });
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

    if (source === "animation" && kind === "frame_patch") {
      if (animationDisabled) {
        emit("animation.patch_dropped", "animation", { reason: "visuals_disabled", ...payload }, "animating");
        return;
      }
      const dropReason = payload.actorTrackId !== activeTrackId
        ? "actor_track_mismatch"
        : payload.trackEpoch !== activeTrackEpoch
          ? "actor_track_epoch_mismatch"
          : payload.frameId !== payload.currentFrameId
            ? "captured_frame_not_current"
            : null;
      if (dropReason) {
        staleLipSyncDropCount += 1;
        emit("animation.patch_dropped", "animation", { reason: dropReason, ...payload }, "animating");
        return;
      }
      if (stageStatus.get("animating") === "pending") transition("animating", "active");
      emit("animation.patch_applied", "animation", payload, "animating");
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
    if (["cancelled", "failed", "crashed"].includes(terminalStatus)) {
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

    if (fault.kind === "runtime_crash" && fault.target === "runtime") {
      lastFailureMs = clock.nowMs;
      terminalStatus = "crashed";
      emit("runtime.crashed", "runtime", { code: fault.code, generation: generation.value });
      for (const stage of SPINE) {
        if (stageStatus.get(stage) === "active") transition(stage, "cancelled", { reason: "runtime_crash" });
      }
      return;
    }

    terminalStatus = "failed";
    lastFailureMs = clock.nowMs;
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
  if (scenario.turn.inputMode !== "typed") transition("listening", "active");

  // Faults are queued first so a fault at the same virtual instant prevents a mocked
  // provider event from being observed. This ordering is part of the v1 contract.
  for (const fault of scenario.faults) {
    clock.schedule(fault.atMs, `fault:${fault.target}:${fault.kind}`, () => handleFault(fault));
  }
  for (const [index, entry] of scenario.stream.entries()) {
    const scheduledGeneration = entry.generation ?? generation.value;
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
    inputReadyMs,
    firstAudioMs,
    speechEndToFirstAudioMs: speechEndMs !== null && firstAudioMs !== null ? firstAudioMs - speechEndMs : null,
    inputReadyToFirstAudioMs: inputReadyMs !== null && firstAudioMs !== null ? firstAudioMs - inputReadyMs : null,
    failureToRecoveryMs: lastFailureMs !== null && lastRecoveryMs !== null ? lastRecoveryMs - lastFailureMs : null,
    recoveryToFirstAudioMs: lastRecoveryMs !== null && firstAudioMs !== null ? firstAudioMs - lastRecoveryMs : null,
    networkAttempts,
    deliveryCommitCount,
    rejectedDeliveryCommitCount,
    manualRetryCount,
    runtimeRestartCount,
    identityAmbiguityCount,
    identityMissCount,
    staleLipSyncDropCount,
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
function evaluateExpected(scenario, result) {
  /** @type {{name: string, expected: unknown, actual: unknown, passed: boolean}[]} */
  const assertions = [];
  const record = (name, expected, actual) => assertions.push({
    name,
    expected,
    actual,
    passed: JSON.stringify(expected) === JSON.stringify(actual),
  });

  record("status", scenario.expected.status, result.status);
  if (scenario.expected.traceSha256) record("traceSha256", scenario.expected.traceSha256, result.traceSha256);
  if (scenario.expected.firstAudioAtMs !== undefined) record("firstAudioAtMs", scenario.expected.firstAudioAtMs, result.metrics.firstAudioMs);
  if (scenario.expected.speechEndToFirstAudioMs !== undefined) {
    record("speechEndToFirstAudioMs", scenario.expected.speechEndToFirstAudioMs, result.metrics.speechEndToFirstAudioMs);
  }
  if (scenario.expected.networkAttempts !== undefined) record("networkAttempts", scenario.expected.networkAttempts, result.metrics.networkAttempts);
  if (scenario.expected.degradationReasons) {
    record("degradationReasons", [...scenario.expected.degradationReasons].sort(), result.degradationReasons);
  }
  for (const [name, expected] of Object.entries(scenario.expected.metrics || {})) {
    record(`metrics.${name}`, expected, result.metrics[name]);
  }
  for (const [kind, expected] of Object.entries(scenario.expected.eventCounts || {})) {
    record(`eventCounts.${kind}`, expected, result.events.filter((event) => event.kind === kind).length);
  }
  for (const [kind, minimum] of Object.entries(scenario.expected.minimumEventCounts || {})) {
    const actual = result.events.filter((event) => event.kind === kind).length;
    assertions.push({ name: `minimumEventCounts.${kind}`, expected: minimum, actual, passed: actual >= minimum });
  }
  for (const [stage, expected] of Object.entries(scenario.expected.spine || {})) {
    record(`spine.${stage}`, expected, result.spine[stage]);
  }
  return assertions;
}

/** @param {any} scenario @param {ReturnType<typeof simulate>} result */
function assertExpected(scenario, result) {
  const failures = evaluateExpected(scenario, result).filter((assertion) => !assertion.passed);
  if (failures.length > 0) {
    throw new Error(`${scenario.id} expectation failure:\n- ${failures.map((failure) => `${failure.name}: expected ${JSON.stringify(failure.expected)}, received ${JSON.stringify(failure.actual)}`).join("\n- ")}`);
  }
}

module.exports = { EVENT_VERSION, SPINE, simulate, evaluateExpected, assertExpected };
