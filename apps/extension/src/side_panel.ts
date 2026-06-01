const captureSelectedTextButton = document.querySelector<HTMLButtonElement>(
  "[data-capture-selected-text]",
);

captureSelectedTextButton?.addEventListener("click", () => {
  void chrome.tabs.query({ active: true, currentWindow: true });
});

export {};
