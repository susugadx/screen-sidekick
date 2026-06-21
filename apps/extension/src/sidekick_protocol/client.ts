export {
  SidekickProtocolClient as SidekickProtocolClientCore,
  createMessageSendIdempotencyKey,
  isMessageSendRequestTimeoutError,
  isTerminalMessageSendReplayError,
} from "./core.js";
export type {
  ProtocolTransport,
  SidekickClient,
  SidekickRequestMethod,
} from "./core.js";
export {
  WebSocketSidekickClient,
  WebSocketSidekickClient as SidekickProtocolClient,
  buildDaemonCaptureUrl,
  buildDaemonWebSocketUrl,
} from "./websocket_client.js";
export {
  NATIVE_CONNECTION_SETTINGS,
  NATIVE_HOST_NAME,
  NativeMessagingSidekickClient,
  isNativeConnectionSettings,
} from "./native_client.js";
