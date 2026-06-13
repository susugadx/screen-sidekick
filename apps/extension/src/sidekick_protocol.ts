export { SidekickProtocolClient, buildDaemonWebSocketUrl } from "./sidekick_protocol/client.js";
export { parseSidekickNotification, parseWireMessageText } from "./sidekick_protocol/parser.js";
export {
  SIDEKICK_PROTOCOL_VERSION,
  SidekickProtocolError,
} from "./sidekick_protocol/types.js";
export type {
  CaptureReason,
  DaemonSettings,
  ErrorCode,
  MessageMode,
  MessageRole,
  MessageSendResult,
  MessageStatus,
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
