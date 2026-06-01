chrome.runtime.onInstalled.addListener(() => {
  void enableSidePanelOnActionClick();
});

chrome.runtime.onStartup.addListener(() => {
  void enableSidePanelOnActionClick();
});

async function enableSidePanelOnActionClick(): Promise<void> {
  if (!chrome.sidePanel?.setPanelBehavior) {
    return;
  }

  try {
    await chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true });
  } catch {
    // 古い Chromium では sidePanel があっても behavior hook が失敗することがある。
  }
}

export {};
