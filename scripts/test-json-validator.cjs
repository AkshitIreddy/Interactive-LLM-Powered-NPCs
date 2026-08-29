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
  const manifestPath = path.join(fixtureRoot, "json-paths-v1.json");

  fs.writeFileSync(validPath, '{"valid":true}\n', "utf8");
  fs.writeFileSync(bomPath, '\ufeff{"valid":"bom"}\n', "utf8");
  fs.writeFileSync(invalidPath, '{"invalid":}\n', "utf8");

  const validResult = spawnSync(process.execPath, [validatorPath, validPath, bomPath], {
    cwd: repositoryRoot,
    encoding: "utf8",
  });
  assert.equal(validResult.status, 0, validResult.stderr || validResult.stdout);
  assert.equal(validResult.stderr, "");

  fs.writeFileSync(
    manifestPath,
    `${JSON.stringify({ version: 1, paths: [validPath, bomPath] })}\n`,
    "utf8",
  );
  const manifestResult = spawnSync(
    process.execPath,
    [validatorPath, "--paths-file", manifestPath],
    {
      cwd: repositoryRoot,
      encoding: "utf8",
    },
  );
  assert.equal(manifestResult.status, 0, manifestResult.stderr || manifestResult.stdout);
  assert.equal(manifestResult.stderr, "");

  const invalidResult = spawnSync(process.execPath, [validatorPath, invalidPath], {
    cwd: repositoryRoot,
    encoding: "utf8",
  });
  assert.equal(invalidResult.status, 1, "invalid JSON must fail validation");
  assert.match(invalidResult.stderr, /invalid document\.json:/);

  const longCorpusRoot = path.join(fixtureRoot, "long-corpus");
  fs.mkdirSync(longCorpusRoot);
  const longCorpus = Array.from({ length: 384 }, (_, index) => {
    const filePath = path.join(
      longCorpusRoot,
      `${String(index).padStart(4, "0")}-${"x".repeat(96)}.json`,
    );
    fs.writeFileSync(filePath, `{\"index\":${index}}\n`, "utf8");
    return filePath;
  });
  const directCommandLength = [process.execPath, validatorPath, ...longCorpus].join(" ").length;
  assert.ok(
    directCommandLength > 32_767,
    `fixture must exceed the Windows command-line limit, got ${directCommandLength} characters`,
  );
  fs.writeFileSync(
    manifestPath,
    `${JSON.stringify({ version: 1, paths: longCorpus })}\n`,
    "utf8",
  );
  const longCorpusResult = spawnSync(
    process.execPath,
    [validatorPath, "--paths-file", manifestPath],
    {
      cwd: repositoryRoot,
      encoding: "utf8",
    },
  );
  assert.equal(longCorpusResult.status, 0, longCorpusResult.stderr || longCorpusResult.stdout);

  fs.writeFileSync(manifestPath, '{"version":2,"paths":[]}\n', "utf8");
  const unsupportedManifestResult = spawnSync(
    process.execPath,
    [validatorPath, "--paths-file", manifestPath],
    {
      cwd: repositoryRoot,
      encoding: "utf8",
    },
  );
  assert.equal(unsupportedManifestResult.status, 2, "unknown manifest versions must fail closed");
  assert.match(unsupportedManifestResult.stderr, /manifest version 1/);

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
  assert.match(
    devScript,
    /@\(\$jsonValidator, '--paths-file', \$jsonManifestPath\)/,
    "dev.ps1 must pass the repository scan through a bounded response-file command",
  );
  assert.doesNotMatch(
    devScript,
    /\$jsonArguments\s*\+=/,
    "dev.ps1 must not append every JSON path to the Windows process argument list",
  );

  console.log("Repository JSON validator regression checks passed.");
} finally {
  fs.rmSync(fixtureRoot, { recursive: true, force: true });
}
