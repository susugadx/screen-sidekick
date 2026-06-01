chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type !== "screen-sidekick:capture-selected-text") {
    return false;
  }

  const selectedText = window.getSelection()?.toString() ?? "";
  sendResponse({ selectedText });
  return false;
});

export {};
