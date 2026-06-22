import { installChrome } from "./side_panel_chrome_harness.mjs";
import { installDom } from "./side_panel_dom_harness.mjs";
import {
  FakeNativePort,
  FakeSidekickServer,
  FakeWebSocket,
} from "./side_panel_fake_server.mjs";

export * from "./side_panel_chrome_harness.mjs";
export * from "./side_panel_dom_harness.mjs";
export * from "./side_panel_fake_server.mjs";
export * from "./side_panel_fixtures.mjs";

export function installSidePanelHarness(options = {}) {
  installDom();
  installChrome(options.storage);
  const server = new FakeSidekickServer(options);

  if (options.native) {
    chrome.runtime.id = "abcdefghijklmnopabcdefghijklmnop";
    chrome.runtime.connectNative = (name) => {
      if (name !== "com.screen_sidekick.host") {
        throw new Error(`unexpected native host name: ${name}`);
      }
      return new FakeNativePort(server);
    };
  }

  globalThis.WebSocket = class extends FakeWebSocket {
    constructor(url) {
      super(url, server);
    }
  };
  globalThis.WebSocket.CONNECTING = FakeWebSocket.CONNECTING;
  globalThis.WebSocket.OPEN = FakeWebSocket.OPEN;
  globalThis.WebSocket.CLOSED = FakeWebSocket.CLOSED;

  return server;
}
