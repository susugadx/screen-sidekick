import assert from "node:assert/strict";
import test from "node:test";

import {
  clearActiveChatMarker,
  saveActiveChatMarker,
} from "../dist/side_panel_active_chat.js";

test("concurrent active chat saves preserve sibling scoped markers", async () => {
  const { locks, storage } = installActiveChatStorage();
  const firstMarker = activeChatMarker();
  const secondMarker = activeChatMarker({
    tabId: 8,
    origin: "https://other.test",
    sessionId: "sess_other",
    activeTurnId: "turn_other",
  });

  await Promise.all([
    saveActiveChatMarker(firstMarker),
    saveActiveChatMarker(secondMarker),
  ]);

  assert.equal(storage.activeChat.version, 1);
  assert.deepEqual(activeChatMarkerFor(storage.activeChat, firstMarker), firstMarker);
  assert.deepEqual(activeChatMarkerFor(storage.activeChat, secondMarker), secondMarker);
  assert.equal(locks.maxConcurrentActive, 1);
});

test("concurrent active chat clear and save do not resurrect cleared sibling markers", async () => {
  const currentMarker = activeChatMarker();
  const siblingMarker = activeChatMarker({
    tabId: 8,
    origin: "https://other.test",
    sessionId: "sess_other",
    activeTurnId: "turn_other",
  });
  const { locks, storage } = installActiveChatStorage({
    activeChat: activeChatMap(currentMarker),
  });

  await Promise.all([
    clearActiveChatMarker({
      tabId: currentMarker.tabId,
      origin: currentMarker.origin,
    }),
    saveActiveChatMarker(siblingMarker),
  ]);

  assert.equal(storage.activeChat.version, 1);
  assert.equal(activeChatMarkerFor(storage.activeChat, currentMarker), null);
  assert.deepEqual(activeChatMarkerFor(storage.activeChat, siblingMarker), siblingMarker);
  assert.equal(locks.maxConcurrentActive, 1);
});

function installActiveChatStorage(initialStorage = {}) {
  const storage = { ...initialStorage };
  const locks = new FakeLockManager();

  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: {
      locks,
    },
  });
  globalThis.chrome = {
    storage: {
      session: {
        async get(keys) {
          const result = {};
          for (const key of keys) {
            if (Object.hasOwn(storage, key)) {
              result[key] = cloneStorageValue(storage[key]);
            }
          }
          return result;
        },
        async set(values) {
          for (const [key, value] of Object.entries(values)) {
            storage[key] = cloneStorageValue(value);
          }
        },
        async remove(keys) {
          for (const key of Array.isArray(keys) ? keys : [keys]) {
            delete storage[key];
          }
        },
      },
    },
  };

  return { locks, storage };
}

class FakeLockManager {
  constructor() {
    this.activeCount = 0;
    this.maxConcurrentActive = 0;
    this.queues = new Map();
  }

  request(name, callback) {
    const previous = this.queues.get(name) ?? Promise.resolve();
    const next = previous.then(async () => {
      this.activeCount += 1;
      this.maxConcurrentActive = Math.max(this.maxConcurrentActive, this.activeCount);
      try {
        return await callback(null);
      } finally {
        this.activeCount -= 1;
      }
    });
    this.queues.set(
      name,
      next.catch(() => {}),
    );
    return next;
  }
}

function activeChatMarker(overrides = {}) {
  return {
    daemonUrl: "http://127.0.0.1:43001",
    daemonToken: "pairing-token",
    tabId: 7,
    origin: "https://example.test",
    sessionId: "sess_1",
    activeTurnId: "turn_1",
    ...overrides,
  };
}

function activeChatMap(...markers) {
  return {
    version: 1,
    markers: Object.fromEntries(
      markers.map((marker) => [activeChatScopeKey(marker.tabId, marker.origin), marker]),
    ),
  };
}

function activeChatMarkerFor(activeChat, markerScope) {
  return (
    activeChat?.markers?.[activeChatScopeKey(markerScope.tabId, markerScope.origin)] ?? null
  );
}

function activeChatScopeKey(tabId, origin) {
  return `tab:${tabId}|origin:${origin}`;
}

function cloneStorageValue(value) {
  return structuredClone(value);
}
