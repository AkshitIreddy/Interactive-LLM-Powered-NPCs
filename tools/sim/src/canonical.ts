// @ts-check

const { createHash } = require("node:crypto");

/** @param {unknown} value */
function canonicalize(value) {
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }

  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .filter(([, child]) => child !== undefined)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, child]) => [key, canonicalize(child)]),
    );
  }

  if (typeof value === "number" && !Number.isFinite(value)) {
    throw new TypeError("Canonical JSON cannot encode a non-finite number");
  }

  return value;
}

/** @param {unknown} value */
function canonicalJson(value) {
  return JSON.stringify(canonicalize(value));
}

/** @param {unknown[]} events */
function canonicalJsonLines(events) {
  return `${events.map(canonicalJson).join("\n")}\n`;
}

/** @param {string | Buffer} value */
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

module.exports = { canonicalize, canonicalJson, canonicalJsonLines, sha256 };
