export {
  NATIVE_CONNECTION_SETTINGS,
  NATIVE_HOST_NAME,
  NativeMessagingSidekickClient,
  SidekickProtocolClient,
  WebSocketSidekickClient,
  buildDaemonCaptureUrl,
  buildDaemonWebSocketUrl,
  createMessageSendIdempotencyKey,
  isNativeConnectionSettings,
  isMessageSendRequestTimeoutError,
  isTerminalMessageSendReplayError,
} from "./sidekick_protocol/client.js";
export type { SidekickClient } from "./sidekick_protocol/client.js";
export {
  parseInitializeResult,
  parseSidekickNotification,
  parseWireMessageText,
} from "./sidekick_protocol/parser.js";
export {
  SIDEKICK_PROTOCOL_VERSION,
  SidekickProtocolError,
} from "./sidekick_protocol/types.js";
export type {
  CaptureReason,
  CodexReadiness,
  DaemonSettings,
  ErrorCode,
  InitializeResult,
  MessageSendIdempotencyDisposition,
  MessageMode,
  MessageRole,
  MessageSendResult,
  MessageStatus,
  ProtocolLimits,
  SafetyStatus,
  SidekickAttachment,
  SidekickMessage,
  SidekickNotification,
  SidekickSession,
  SidekickSessionSnapshot,
  SidekickTurn,
  TurnStatus,
  WireMessage,
} from "./sidekick_protocol/types.js";
