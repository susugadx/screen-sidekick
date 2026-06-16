import { CURRENT_ORIGIN, CURRENT_TAB_ID } from "./side_panel_fixtures.mjs";

export function installChrome(initialStorage = {}) {
  const storage = {
    daemonSettings: {
      url: "http://127.0.0.1:43001",
      token: "pairing-token",
    },
    ...initialStorage,
  };
  globalThis.chrome = {
    runtime: {
      getManifest() {
        return { version: "test" };
      },
    },
    storage: {
      session: {
        async get(keys) {
          if (!Array.isArray(keys)) {
            return { ...storage };
          }
          const result = {};
          for (const key of keys) {
            if (Object.hasOwn(storage, key)) {
              result[key] = storage[key];
            }
          }
          return result;
        },
        async set(values) {
          Object.assign(storage, values);
        },
        async remove(keys) {
          for (const key of Array.isArray(keys) ? keys : [keys]) {
            delete storage[key];
          }
        },
      },
    },
    tabs: {
      async query() {
        return [
          {
            id: CURRENT_TAB_ID,
            url: `${CURRENT_ORIGIN}/admin`,
            title: "Admin",
            windowId: 1,
          },
        ];
      },
      async captureVisibleTab() {
        throw new Error("Either the '<all_urls>' or 'activeTab' permission is required.");
      },
    },
    scripting: {
      async executeScript() {
        return [
          {
            result: {
              selectedText: "",
              buttons: [],
              inputs: [],
            },
          },
        ];
      },
    },
    permissions: {
      async request() {
        return true;
      },
    },
  };
}

export class FakeLockManager {
  constructor() {
    this.queues = new Map();
  }

  request(name, callback) {
    const previous = this.queues.get(name) ?? Promise.resolve();
    const next = previous.then(() => callback(null));
    this.queues.set(
      name,
      next.catch(() => {}),
    );
    return next;
  }
}
