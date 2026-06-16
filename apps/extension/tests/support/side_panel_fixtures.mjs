export const CURRENT_TAB_ID = 7;
export const CURRENT_ORIGIN = "https://example.test";

export function activeChatStorage(activeChat) {
  return {
    daemonSettings: {
      url: "http://127.0.0.1:43001",
      token: "pairing-token",
    },
    activeChat,
  };
}

export function scopedActiveChatMarker(overrides = {}) {
  return {
    daemonUrl: "http://127.0.0.1:43001",
    daemonToken: "pairing-token",
    tabId: CURRENT_TAB_ID,
    origin: CURRENT_ORIGIN,
    sessionId: "sess_1",
    activeTurnId: "turn_1",
    ...overrides,
  };
}

export function scopedActiveChatMarkerWithoutTurn() {
  const { activeTurnId: _activeTurnId, ...marker } = scopedActiveChatMarker();
  return marker;
}

export function activeChatMap(...markers) {
  return {
    version: 1,
    markers: Object.fromEntries(
      markers.map((marker) => [activeChatScopeKey(marker.tabId, marker.origin), marker]),
    ),
  };
}

export function currentActiveChatMarker(activeChat) {
  return activeChatMarkerFor(activeChat, {
    tabId: CURRENT_TAB_ID,
    origin: CURRENT_ORIGIN,
  });
}

export function activeChatMarkerFor(activeChat, markerScope) {
  return (
    activeChat?.markers?.[activeChatScopeKey(markerScope.tabId, markerScope.origin)] ?? null
  );
}

export function activeChatScopeKey(tabId, origin) {
  return `tab:${tabId}|origin:${origin}`;
}

export function legacyActiveChatMarker() {
  const { tabId: _tabId, origin: _origin, ...legacyMarker } = scopedActiveChatMarker();
  return legacyMarker;
}

export function completedActiveChatSessions(userText) {
  return {
    sess_1: {
      session: {
        id: "sess_1",
        title: "Screen Sidekick",
      },
      messages: [
        {
          id: "msg_old",
          session_id: "sess_1",
          role: "user",
          text: userText,
          status: "completed",
          turn_id: "turn_old",
        },
      ],
      attachments: [],
      active_turn: null,
    },
  };
}

export function runningActiveChatSessions(userText = null) {
  const messages = userText
    ? [
        {
          id: "msg_1",
          session_id: "sess_1",
          role: "user",
          text: userText,
          status: "pending",
          turn_id: "turn_1",
        },
      ]
    : [];
  return {
    sess_1: {
      session: {
        id: "sess_1",
        title: "Screen Sidekick",
      },
      messages,
      attachments: [],
      active_turn: {
        id: "turn_1",
        session_id: "sess_1",
        status: "running",
      },
    },
  };
}

export function terminalActiveChatSessions(userText, status, turnId = "turn_1") {
  return {
    sess_1: {
      session: {
        id: "sess_1",
        title: "Screen Sidekick",
      },
      messages: [
        {
          id: "msg_1",
          session_id: "sess_1",
          role: "user",
          text: userText,
          status,
          turn_id: turnId,
        },
      ],
      attachments: [],
      active_turn: null,
    },
  };
}

export function captureBridgeResponse() {
  return {
    schema_version: "sidekick_capture_bridge.v0.1",
    screen_context_json: "{}",
    prompt_text: "Prompt",
    safety: {
      has_danger: false,
      warning_count: 0,
      warnings: [],
      masked_input_values: 0,
      masked_secret_texts: 0,
    },
  };
}
