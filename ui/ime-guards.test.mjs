import test from "node:test";
import assert from "node:assert/strict";

import {
  isImeComposing,
  shouldTranslateOnInput,
  shouldTranslateOnEnter,
} from "./ime-guards.mjs";

test("组合输入中的 input 事件不触发翻译", () => {
  // Chromium: the composition buffer arrives with isComposing === true.
  assert.equal(shouldTranslateOnInput({ isComposing: true }, false), false);
  // Engines without the flag fall back to keyCode 229.
  assert.equal(shouldTranslateOnInput({ keyCode: 229 }, false), false);
  // compositionstart already seen: trust the tracked state.
  assert.equal(shouldTranslateOnInput({ isComposing: false }, true), false);
});

test("已上屏文本的 input 事件照常触发翻译", () => {
  assert.equal(shouldTranslateOnInput({ isComposing: false, keyCode: 0 }, false), true);
  assert.equal(shouldTranslateOnInput({ data: "你好" }, false), true);
});

test("组合输入期间的回车交给输入法确认候选词，不当作翻译指令", () => {
  assert.equal(shouldTranslateOnEnter({ key: "Enter", isComposing: true }, false), false);
  assert.equal(shouldTranslateOnEnter({ key: "Enter", keyCode: 229 }, false), false);
  assert.equal(shouldTranslateOnEnter({ key: "Enter" }, true), false);
});

test("非组合状态下的回车触发翻译，Shift+Enter 仍然换行", () => {
  assert.equal(shouldTranslateOnEnter({ key: "Enter", isComposing: false }, false), true);
  assert.equal(shouldTranslateOnEnter({ key: "Enter", shiftKey: true }, false), false);
  assert.equal(shouldTranslateOnEnter({ key: "a" }, false), false);
});

test("isImeComposing 兼容事件标记与旧版 keyCode", () => {
  assert.equal(isImeComposing(undefined, false), false);
  assert.equal(isImeComposing({}, false), false);
  assert.equal(isImeComposing({ isComposing: true }, false), true);
  assert.equal(isImeComposing({ keyCode: 229 }, false), true);
  assert.equal(isImeComposing({}, true), true);
});
