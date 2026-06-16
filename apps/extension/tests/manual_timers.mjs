import assert from "node:assert/strict";

export function installManualTimers() {
  const previousSetTimeout = globalThis.setTimeout;
  const previousClearTimeout = globalThis.clearTimeout;
  let nextId = 1;
  const timers = new Map();

  globalThis.setTimeout = (callback, delay, ...args) => {
    const id = nextId;
    nextId += 1;
    timers.set(id, {
      callback: () => callback(...args),
      delay,
    });
    return id;
  };
  globalThis.clearTimeout = (id) => {
    timers.delete(id);
  };

  return {
    get size() {
      return timers.size;
    },
    nextDelay() {
      const entry = timers.values().next().value;
      assert.ok(entry, "expected pending timer");
      return entry.delay;
    },
    fireNext() {
      const entry = timers.entries().next().value;
      assert.ok(entry, "expected pending timer");
      const [id, timer] = entry;
      timers.delete(id);
      timer.callback();
    },
    restore() {
      globalThis.setTimeout = previousSetTimeout;
      globalThis.clearTimeout = previousClearTimeout;
    },
  };
}

export async function flushMicrotasks(count = 10) {
  for (let index = 0; index < count; index += 1) {
    await Promise.resolve();
  }
}

export async function waitForMicrotasks(predicate, count = 100) {
  for (let index = 0; index < count; index += 1) {
    if (predicate()) {
      return;
    }
    await Promise.resolve();
  }
  assert.fail("microtask condition was not met");
}
