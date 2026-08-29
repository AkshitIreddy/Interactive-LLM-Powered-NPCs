"use strict";

const fs = require("node:fs");

const MAX_MANIFEST_BYTES = 16 * 1024 * 1024;

function readUtf8(filePath) {
  let source = fs.readFileSync(filePath, "utf8");
  if (source.charCodeAt(0) === 0xfeff) {
    source = source.slice(1);
  }
  return source;
}

function readPaths(argumentsList) {
  if (argumentsList[0] !== "--paths-file") {
    return argumentsList;
  }

  if (argumentsList.length !== 2) {
    throw new Error("--paths-file requires exactly one response manifest path");
  }

  const manifestPath = argumentsList[1];
  const manifestSize = fs.statSync(manifestPath).size;
  if (manifestSize > MAX_MANIFEST_BYTES) {
    throw new Error(`response manifest exceeds ${MAX_MANIFEST_BYTES} bytes`);
  }

  const manifest = JSON.parse(readUtf8(manifestPath));
  if (
    manifest === null ||
    Array.isArray(manifest) ||
    typeof manifest !== "object" ||
    manifest.version !== 1 ||
    !Array.isArray(manifest.paths)
  ) {
    throw new Error("response manifest must use manifest version 1 with a paths array");
  }

  const unexpectedKeys = Object.keys(manifest).filter(
    (key) => key !== "version" && key !== "paths",
  );
  if (unexpectedKeys.length > 0) {
    throw new Error(`response manifest contains unknown field: ${unexpectedKeys[0]}`);
  }

  for (const filePath of manifest.paths) {
    if (typeof filePath !== "string" || filePath.length === 0 || filePath.includes("\0")) {
      throw new Error("response manifest paths must be non-empty strings without NUL bytes");
    }
  }
  return manifest.paths;
}

let filePaths;
try {
  filePaths = readPaths(process.argv.slice(2));
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`JSON path manifest: ${message}`);
  process.exitCode = 2;
  return;
}

let invalid = false;
for (const filePath of filePaths) {
  try {
    JSON.parse(readUtf8(filePath));
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`${filePath}: ${message}`);
    invalid = true;
  }
}

process.exitCode = invalid ? 1 : 0;
