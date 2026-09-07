// IME composition guards for the text translation input.
//
// While an IME composition is active the textarea does not hold the text the
// user means: it holds the raw buffer ("nihao" / "apple"), which is still
// underlined and can be replaced by the candidate list. Chromium / WebView2
// nevertheless fires `input` events during the composition, so a plain
// "translate on input" debounce ends up translating the unfinished buffer.
//
// The Enter keystroke is worse: the key used to confirm a candidate reaches the
// page as a normal `keydown` with `isComposing === true` (or `keyCode === 229`
// on engines that do not expose the flag). Calling preventDefault() on it both
// swallows the IME commit and fires a translation of the raw buffer, so
// Chinese / Japanese / Korean — and Microsoft Pinyin's own English mode — users
// have to press Enter one extra time before they see a sane result.
//
// Both cases are ignored here so that typing commits normally and the
// translation always runs on committed text only.

export function isImeComposing(event, composing) {
  if (composing) return true;
  if (!event) return false;
  if (event.isComposing) return true;
  // Legacy / engine fallback for "this keystroke belongs to the IME".
  return event.keyCode === 229;
}

// Only schedule the debounced auto-translate for `input` events that carry
// committed text, never for the raw composition buffer.
export function shouldTranslateOnInput(event, composing) {
  return !isImeComposing(event, composing);
}

// Enter translates only when the keystroke is not the IME's "confirm
// candidate". Shift+Enter keeps inserting a newline as before.
export function shouldTranslateOnEnter(event, composing) {
  if (!event || isImeComposing(event, composing)) return false;
  return event.key === "Enter" && !event.shiftKey;
}
