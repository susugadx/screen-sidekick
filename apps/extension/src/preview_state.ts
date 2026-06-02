export interface PreviewStateElements {
  screenContextJson: HTMLTextAreaElement;
  promptText: HTMLTextAreaElement;
  safetySummary: HTMLPreElement;
  copyJson: HTMLButtonElement;
  copyPrompt: HTMLButtonElement;
}

export interface PreviewContent {
  screenContextJson: string;
  promptText: string;
  safetySummaryText: string;
}

export function clearPreviewState(elements: PreviewStateElements): void {
  elements.screenContextJson.value = "";
  elements.promptText.value = "";
  elements.safetySummary.textContent = "";
  elements.copyJson.disabled = true;
  elements.copyPrompt.disabled = true;
}

export function setPreviewState(
  elements: PreviewStateElements,
  content: PreviewContent,
): void {
  elements.screenContextJson.value = content.screenContextJson;
  elements.promptText.value = content.promptText;
  elements.safetySummary.textContent = content.safetySummaryText;
  elements.copyJson.disabled = false;
  elements.copyPrompt.disabled = false;
}
