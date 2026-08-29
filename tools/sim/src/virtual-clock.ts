// @ts-check

class VirtualClock {
  constructor() {
    this.nowMs = 0;
    this.nextOrder = 0;
    /** @type {{atMs: number, order: number, label: string, callback: () => void}[]} */
    this.queue = [];
  }

  /** @param {number} atMs @param {string} label @param {() => void} callback */
  schedule(atMs, label, callback) {
    if (!Number.isSafeInteger(atMs) || atMs < this.nowMs) {
      throw new RangeError(`Cannot schedule ${label} at invalid virtual time ${atMs}`);
    }
    this.queue.push({ atMs, order: this.nextOrder++, label, callback });
  }

  run() {
    while (this.queue.length > 0) {
      this.queue.sort((left, right) => left.atMs - right.atMs || left.order - right.order);
      const next = this.queue.shift();
      if (!next) break;
      this.nowMs = next.atMs;
      next.callback();
    }
  }
}

class SeededRandom {
  /** @param {number} seed */
  constructor(seed) {
    this.state = seed >>> 0;
  }

  next() {
    let value = (this.state += 0x6d2b79f5);
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  }

  /** @param {number} maximumExclusive */
  integer(maximumExclusive) {
    if (!Number.isSafeInteger(maximumExclusive) || maximumExclusive <= 0) {
      throw new RangeError("maximumExclusive must be a positive integer");
    }
    return Math.floor(this.next() * maximumExclusive);
  }
}

module.exports = { VirtualClock, SeededRandom };
