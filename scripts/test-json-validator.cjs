"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const scriptsDirectory = __dirname;
const repositoryRoot = path.resolve(scriptsDirectory, "..");
const validatorPath = path.join(scriptsDirectory, "validate-json.cjs");
const devScriptPath = path.join(scriptsDirectory, "dev.ps1");
const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "npc-json-validator-"));

try {
  const validPath = path.join(fixtureRoot, "valid document.json");
  const bomPath = path.join(fixtureRoot, "utf8-bom.json");
  const invalidPath = path.join(fixtureRoot, "invalid document.json");

  fs.writeFileSync(validPath, '{"valid":true}\n', "utf8");
  fs.writeFileSync(bomPath, '\ufeff{"valid":"bom"}\n', "utf8");
  fs.writeFileSync(invalidPath, '{"invalid":}\n', "utf8");

  const validResult = spawnSync(process.execPath, [validatorPath, validPath, bomPath], {
    cwd: repositoryRoot,
    encoding: "utf8",
  });
  assert.equal(validResult.status, 0, validResult.stderr || validResult.stdout);
  assert.equal(validResult.stderr, "");

  const invalidResult = spawnSync(process.execPath, [validatorPath, invalidPath], {
    cwd: repositoryRoot,
    encoding: "utf8",
  });
  assert.equal(invalidResult.status, 1, "invalid JSON must fail validation");
  assert.match(invalidResult.stderr, /invalid document\.json:/);

  const devScript = fs.readFileSync(devScriptPath, "utf8");
  assert.match(
    devScript,
    /Join-Path \$PSScriptRoot 'validate-json\.cjs'/,
    "dev.ps1 must invoke the checked-in validator file",
  );
  assert.doesNotMatch(
    devScript,
    /\$nodeScript\s*=|@\('-e',\s*\$nodeScript/,
    "dev.ps1 must not pass quote-sensitive JavaScript through node -e",
  );

  console.log("Repository JSON validator regression checks passed.");
} finally {
  fs.rmSync(fixtureRoot, { recursive: true, force: true });
}
