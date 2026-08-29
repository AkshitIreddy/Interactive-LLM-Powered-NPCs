// @ts-check

const assert = require("node:assert/strict");
const test = require("node:test");
const { canonicalJson, canonicalJsonLines, sha256 } = require("../src/canonical.ts");
const { VirtualClock, SeededRandom } = require("../src/virtual-clock.ts");

test("canonical JSON recursively sorts keys and omits undefined object fields", () => {
  const actual = canonicalJson({ z: 1, a: { d: undefined, c: 3, b: 2 }, list: [{ y: 2, x: 1 }] });
  assert.equal(actual, '{"a":{"b":2,"c":3},"list":[{"x":1,"y":2}],"z":1}');
});

test("canonical JSON Lines has one trailing newline and a stable digest", () => {
  const jsonl = canonicalJsonLines([{ b: 2, a: 1 }, { d: 4, c: 3 }]);
  assert.equal(jsonl, '{"a":1,"b":2}\n{"c":3,"d":4}\n');
  assert.equal(sha256(jsonl), "5920a6f20611f67bc96a2e218c799765e136fd39b00ff43766969a85fee60cd0");
});

test("virtual clock orders time first and insertion order second", () => {
  const clock = new VirtualClock();
  const observed = [];
  clock.schedule(20, "late", () => observed.push(`late@${clock.nowMs}`));
  clock.schedule(10, "first", () => observed.push(`first@${clock.nowMs}`));
  clock.schedule(10, "second", () => observed.push(`second@${clock.nowMs}`));
  clock.run();
  assert.deepEqual(observed, ["first@10", "second@10", "late@20"]);
  assert.throws(() => clock.schedule(19, "past", () => {}), /invalid virtual time/);
});

test("seeded random streams replay exactly and diverge with a different seed", () => {
  const left = new SeededRandom(42);
  const replay = new SeededRandom(42);
  const other = new SeededRandom(43);
  const sample = (random) => Array.from({ length: 8 }, () => random.integer(10000));
  const first = sample(left);
  assert.deepEqual(first, sample(replay));
  assert.notDeepEqual(first, sample(other));
});
