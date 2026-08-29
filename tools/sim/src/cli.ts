#!/usr/bin/env node
// @ts-check

const fs = require("node:fs");
const path = require("node:path");
const { canonicalJson } = require("./canonical.ts");
const {
  LEDGER_VERSION,
  fixtureRoot,
  readJson,
  loadScenarios,
  loadScenario,
  computeFixtureLedger,
} = require("./manifest.ts");
const { simulate, assertExpected } = require("./simulator.ts");
const { buildBenchmarkReport } = require("./benchmark.ts");

function usage() {
  return [
    "NPC 2.0 deterministic simulation harness",
    "",
    "Usage:",
    "  node src/cli.ts list",
    "  node src/cli.ts run <scenario-id> [--output <file>]",
    "  node src/cli.ts run-all [--output <directory>]",
    "  node src/cli.ts benchmark [--output <file>]",
    "  node src/cli.ts verify",
    "  node src/cli.ts update-hashes",
  ].join("\n");
}

/** @param {string[]} args @param {string} name */
function option(args, name) {
  const index = args.indexOf(name);
  if (index === -1) return null;
  if (!args[index + 1]) throw new Error(`${name} requires a value`);
  return args[index + 1];
}

/** @param {string} filename @param {string} contents */
function writeOutput(filename, contents) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.writeFileSync(filename, contents, "utf8");
}

function verifyLedger() {
  const filename = path.join(fixtureRoot(), "fixture-hashes.v1.json");
  const expected = readJson(filename);
  if (expected.schemaVersion !== LEDGER_VERSION) throw new Error("Fixture ledger has an unsupported schemaVersion");
  const actual = computeFixtureLedger();
  if (canonicalJson(actual) !== canonicalJson(expected)) {
    throw new Error("Fixture integrity ledger is stale. Review fixture changes, then run update-hashes.");
  }
}

function main() {
  const args = process.argv.slice(2);
  const command = args[0] || "help";
  if (command === "help" || command === "--help" || command === "-h") {
    process.stdout.write(`${usage()}\n`);
    return;
  }

  if (command === "list") {
    for (const { scenario } of loadScenarios()) {
      process.stdout.write(`${scenario.id}\t${scenario.description}\n`);
    }
    return;
  }

  if (command === "run") {
    if (!args[1] || args[1].startsWith("--")) throw new Error("run requires a scenario id");
    const { scenario, resourceProfile } = loadScenario(args[1]);
    const result = simulate(scenario, resourceProfile);
    assertExpected(scenario, result);
    const output = option(args, "--output");
    if (output) writeOutput(path.resolve(output), result.jsonl);
    else process.stdout.write(result.jsonl);
    return;
  }

  if (command === "run-all") {
    const output = option(args, "--output");
    for (const { scenario, resourceProfile } of loadScenarios()) {
      const result = simulate(scenario, resourceProfile);
      assertExpected(scenario, result);
      if (output) writeOutput(path.join(path.resolve(output), `${scenario.id}.events.jsonl`), result.jsonl);
      process.stderr.write(`PASS ${scenario.id} ${result.traceSha256}\n`);
    }
    return;
  }

  if (command === "verify") {
    const firstPass = [];
    for (const { scenario, resourceProfile } of loadScenarios()) {
      const result = simulate(scenario, resourceProfile);
      assertExpected(scenario, result);
      const replay = simulate(scenario, resourceProfile);
      if (result.jsonl !== replay.jsonl) throw new Error(`${scenario.id}: replay is not deterministic`);
      firstPass.push({ id: scenario.id, traceSha256: result.traceSha256 });
    }
    verifyLedger();
    process.stdout.write(`${canonicalJson({ status: "ok", scenarios: firstPass })}\n`);
    return;
  }

  if (command === "benchmark") {
    const report = `${canonicalJson(buildBenchmarkReport())}\n`;
    const output = option(args, "--output");
    if (output) writeOutput(path.resolve(output), report);
    else process.stdout.write(report);
    return;
  }

  if (command === "update-hashes") {
    const ledger = computeFixtureLedger();
    const filename = path.join(fixtureRoot(), "fixture-hashes.v1.json");
    fs.writeFileSync(filename, `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
    process.stdout.write(`Updated ${filename}\n`);
    return;
  }

  throw new Error(`Unknown command '${command}'\n\n${usage()}`);
}

try {
  main();
} catch (error) {
  process.stderr.write(`sim: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
}
