// When a language / engine dropdown is changed, the old result on screen was
// produced with the *previous* options, so it is stale: the user had to press
// Enter to get a result for the new selection. Re-run the translation right
// away instead — but only when that can't make things worse than leaving the
// stale (still readable) result in place.

export function canRetranslateNow({ inputText, engine, llmApiKey } = {}) {
  const text = typeof inputText === "string" ? inputText.trim() : "";
  // Nothing typed: there is nothing to translate, and re-running would wipe the
  // current output for no reason.
  if (!text) return false;
  // Switching to the AI engine without a key only produces an inline error,
  // replacing a perfectly good result from the previous engine. The existing
  // "请先在设置中配置 API Key" hint still shows up when the user asks for it.
  if (engine === "llm" && !llmApiKey) return false;
  return true;
}
