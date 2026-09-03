// @ts-check

const fs = require("node:fs");
const path = require("node:path");
const { canonicalJson, sha256 } = require("./canonical.ts");

const SCENARIO_VERSION = "npc.sim.scenario.v1";
const RESOURCE_VERSION = "npc.sim.resource-profile.v1";
const LEDGER_VERSION = "npc.sim.fixture-ledger.v1";
const SOURCES = new Set(["input", "stt", "identity", "memory", "llm", "tts", "animation", "delivery", "control"]);
const STREAM_KINDS = new Set([
  "partial",
  "final",
  "resolved",
  "retrieved",
  "token",
  "sentence",
  "complete",
  "audio_chunk",
  "viseme",
  "barge_in",
  "typed_input",
  "ambiguous",
  "missed",
  "frame_patch",
  "commit",
  "manual_retry",
  "runtime_restart",
]);
const SOURCE_KINDS = new Map([
  ["input", new Set(["typed_input"])],
  ["stt", new Set(["partial", "final"])],
  ["identity", new Set(["resolved", "ambiguous", "missed"])],
  ["memory", new Set(["retrieved"])],
  ["llm", new Set(["token", "sentence", "complete"])],
  ["tts", new Set(["audio_chunk", "complete"])],
  ["animation", new Set(["viseme", "frame_patch", "complete"])],
  ["delivery", new Set(["commit"])],
  ["control", new Set(["barge_in", "manual_retry", "runtime_restart"])],
]);

function findRepoRoot() {
  return path.resolve(__dirname, "../../..");
}

function fixtureRoot() {
  return path.join(findRepoRoot(), "fixtures", "sim");
}

/** @param {string} filename */
function readJson(filename) {
  return JSON.parse(fs.readFileSync(filename, "utf8"));
}

/** @param {unknown} condition @param {string} message */
function requireValue(condition, message) {
  if (!condition) throw new Error(message);
}

/** @param {any} scenario @param {string} [label] */
function validateScenario(scenario, label = "scenario") {
  requireValue(scenario && typeof scenario === "object", `${label}: must be an object`);
  requireValue(scenario.schemaVersion === SCENARIO_VERSION, `${label}: unsupported schemaVersion`);
  requireValue(/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(scenario.id), `${label}: invalid id`);
  requireValue(typeof scenario.description === "string" && scenario.description.length >= 12, `${label}: description is required`);
  requireValue(Number.isSafeInteger(scenario.seed) && scenario.seed >= 0, `${label}: seed must be a non-negative integer`);
  requireValue(scenario.resourceProfile && typeof scenario.resourceProfile === "string", `${label}: resourceProfile is required`);
  requireValue(scenario.turn?.sessionId && scenario.turn?.turnId, `${label}: turn identifiers are required`);
  requireValue(scenario.turn?.npc?.id && scenario.turn?.npc?.name, `${label}: NPC identity is required`);
  requireValue(["visible", "offscreen", "unknown"].includes(scenario.turn?.npc?.visibility), `${label}: invalid NPC visibility`);
  requireValue([undefined, "speech", "typed"].includes(scenario.turn?.inputMode), `${label}: invalid input mode`);
  requireValue(["hosted", "hybrid", "fully_local", "offline"].includes(scenario.privacy?.mode), `${label}: invalid privacy mode`);
  requireValue(Array.isArray(scenario.stream), `${label}: stream must be an array`);
  requireValue(Array.isArray(scenario.faults), `${label}: faults must be an array`);

  let previousAt = -1;
  for (const [index, event] of scenario.stream.entries()) {
    requireValue(Number.isSafeInteger(event.atMs) && event.atMs >= 0, `${label}: stream[${index}] has invalid atMs`);
    requireValue(event.atMs >= previousAt, `${label}: stream must be ordered by atMs`);
    previousAt = event.atMs;
    requireValue(SOURCES.has(event.source), `${label}: stream[${index}] has invalid source`);
    requireValue(STREAM_KINDS.has(event.kind), `${label}: stream[${index}] has invalid kind`);
    requireValue(SOURCE_KINDS.get(event.source)?.has(event.kind), `${label}: stream[${index}] kind is invalid for source`);
    requireValue(event.payload && typeof event.payload === "object" && !Array.isArray(event.payload), `${label}: stream[${index}] payload must be an object`);
    requireValue(event.generation === undefined || (Number.isSafeInteger(event.generation) && event.generation >= 0), `${label}: stream[${index}] has invalid generation`);
  }

  for (const [index, fault] of scenario.faults.entries()) {
    requireValue(Number.isSafeInteger(fault.atMs) && fault.atMs >= 0, `${label}: faults[${index}] has invalid atMs`);
    requireValue(["stt", "identity", "memory", "llm", "tts", "animation", "resource", "runtime"].includes(fault.target), `${label}: faults[${index}] has invalid target`);
    requireValue(["provider_error", "worker_crash", "timeout", "low_vram", "runtime_crash"].includes(fault.kind), `${label}: faults[${index}] has invalid kind`);
    requireValue(typeof fault.code === "string" && fault.code.length > 0, `${label}: faults[${index}] code is required`);
  }

  const providerValues = Object.values(scenario.providers || {});
  if (scenario.privacy.mode === "offline" || scenario.privacy.mode === "fully_local") {
    requireValue(scenario.privacy.networkAllowed === false, `${label}: local/offline modes must disable network`);
    requireValue(providerValues.every((provider) => provider === "mock-local"), `${label}: local/offline mode cannot name a hosted provider`);
  }

  requireValue(["completed", "cancelled", "failed", "degraded"].includes(scenario.expected?.status), `${label}: expected.status is required`);
  return scenario;
}

/** @param {any} profile @param {string} [label] */
function validateResourceProfile(profile, label = "resource profile") {
  requireValue(profile?.schemaVersion === RESOURCE_VERSION, `${label}: unsupported schemaVersion`);
  requireValue(/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(profile.id), `${label}: invalid id`);
  requireValue(["silent", "balanced", "turbo", "unknown"].includes(profile.power.mode), `${label}: invalid power mode`);
  requireValue(typeof profile.power.cpuBoostEnabled === "boolean", `${label}: cpuBoostEnabled is required`);
  requireValue(typeof profile.workload.competingAgents === "boolean", `${label}: competingAgents is required`);
  requireValue(typeof profile.benchmark.canonical === "boolean", `${label}: benchmark.canonical is required`);
  requireValue(Number.isSafeInteger(profile.resources.vramBudgetMiB) && profile.resources.vramBudgetMiB > 0, `${label}: invalid VRAM budget`);
  return profile;
}

function loadResourceProfiles() {
  const directory = path.join(fixtureRoot(), "resources");
  const profiles = new Map();
  for (const name of fs.readdirSync(directory).filter((name) => name.endsWith(".json")).sort()) {
    const filename = path.join(directory, name);
    const profile = validateResourceProfile(readJson(filename), name);
    requireValue(!profiles.has(profile.id), `${name}: duplicate profile id ${profile.id}`);
    profiles.set(profile.id, profile);
  }
  return profiles;
}

function loadScenarios() {
  const directory = path.join(fixtureRoot(), "scenarios");
  const profiles = loadResourceProfiles();
  return fs.readdirSync(directory)
    .filter((name) => name.endsWith(".scenario.v1.json"))
    .sort()
    .map((name) => {
      const filename = path.join(directory, name);
      const scenario = validateScenario(readJson(filename), name);
      requireValue(profiles.has(scenario.resourceProfile), `${name}: unknown resource profile ${scenario.resourceProfile}`);
      return { filename, scenario, resourceProfile: profiles.get(scenario.resourceProfile) };
    });
}

/** @param {string} id */
function loadScenario(id) {
  const result = loadScenarios().find(({ scenario }) => scenario.id === id);
  if (!result) throw new Error(`Unknown scenario '${id}'. Use 'list' to see valid ids.`);
  return result;
}

function fixtureDocuments() {
  const root = fixtureRoot();
  const paths = [];
  for (const directory of ["resources", "scenarios", "schemas"]) {
    const absolute = path.join(root, directory);
    for (const name of fs.readdirSync(absolute).filter((name) => name.endsWith(".json")).sort()) {
      paths.push(path.join(absolute, name));
    }
  }
  return paths;
}

function computeFixtureLedger() {
  const root = fixtureRoot();
  const fixtures = fixtureDocuments().map((filename) => {
    const relativePath = path.relative(root, filename).replaceAll(path.sep, "/");
    const document = readJson(filename);
    return { path: relativePath, sha256: sha256(canonicalJson(document)) };
  });
  return { schemaVersion: LEDGER_VERSION, algorithm: "sha256-canonical-json", fixtures };
}

module.exports = {
  SCENARIO_VERSION,
  RESOURCE_VERSION,
  LEDGER_VERSION,
  fixtureRoot,
  readJson,
  validateScenario,
  validateResourceProfile,
  loadResourceProfiles,
  loadScenarios,
  loadScenario,
  computeFixtureLedger,
};
