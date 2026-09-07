import test from "node:test";
import assert from "node:assert/strict";

import { canRetranslateNow } from "./retranslate.mjs";

test("有已输入文本时，换语言可以立刻重翻", () => {
  assert.equal(
    canRetranslateNow({ inputText: "你好", engine: "bing", llmApiKey: "" }),
    true,
  );
});

test("输入框为空时不重翻，避免白刷一次输出", () => {
  assert.equal(canRetranslateNow({ inputText: "", engine: "bing" }), false);
  assert.equal(canRetranslateNow({ inputText: "   \n ", engine: "bing" }), false);
  assert.equal(canRetranslateNow({ inputText: undefined, engine: "bing" }), false);
});

test("切到 AI 引擎但没配 Key 时不重翻，别把已有结果换成报错", () => {
  assert.equal(
    canRetranslateNow({ inputText: "你好", engine: "llm", llmApiKey: "" }),
    false,
  );
  assert.equal(
    canRetranslateNow({ inputText: "你好", engine: "llm", llmApiKey: "sk-x" }),
    true,
  );
});

test("参数缺失时按不重翻处理", () => {
  assert.equal(canRetranslateNow(), false);
  assert.equal(canRetranslateNow({}), false);
});
