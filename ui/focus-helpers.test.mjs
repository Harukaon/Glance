import test from "node:test";
import assert from "node:assert/strict";

import { focusTextInputIfAllowed, shouldAutoFocusTextInput } from "./focus-helpers.mjs";

test("主窗口且未录制快捷键时允许自动聚焦", () => {
  assert.equal(
    shouldAutoFocusTextInput({ mode: "main", hotkeyRecording: false }),
    true,
  );
});

test("录制快捷键时不自动聚焦", () => {
  assert.equal(
    shouldAutoFocusTextInput({ mode: "main", hotkeyRecording: true }),
    false,
  );
});

test("非主窗口模式不自动聚焦", () => {
  assert.equal(
    shouldAutoFocusTextInput({ mode: "overlay", hotkeyRecording: false }),
    false,
  );
});

test("允许自动聚焦时将焦点放到输入框末尾", () => {
  const calls = [];
  const input = {
    value: "hello",
    focus(options) {
      calls.push(["focus", options]);
    },
    setSelectionRange(start, end) {
      calls.push(["setSelectionRange", start, end]);
    }
  };

  const focused = focusTextInputIfAllowed({
    mode: "main",
    hotkeyRecording: false,
    input,
  });

  assert.equal(focused, true);
  assert.deepEqual(calls, [
    ["focus", { preventScroll: true }],
    ["setSelectionRange", 5, 5],
  ]);
});

test("没有输入框时安全跳过", () => {
  const focused = focusTextInputIfAllowed({
    mode: "main",
    hotkeyRecording: false,
    input: null,
  });

  assert.equal(focused, false);
});
