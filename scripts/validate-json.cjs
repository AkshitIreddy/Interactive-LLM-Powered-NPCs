"use strict";

const fs = require("node:fs");

let invalid = false;

for (const filePath of process.argv.slice(2)) {
  try {
    let source = fs.readFileSync(filePath, "utf8");
    if (source.charCodeAt(0) === 0xfeff) {
      source = source.slice(1);
    }
    JSON.parse(source);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`${filePath}: ${message}`);
    invalid = true;
  }
}

process.exitCode = invalid ? 1 : 0;
