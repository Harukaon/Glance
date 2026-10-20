import test from "node:test";
import assert from "node:assert/strict";

import { recordedBinding, rowClasses, rowState, shouldEnterRecording } from "./hotkey-enable.mjs";

// 开关只把绑定「停掉」，绑定本身原样留着 —— 关掉时 combo 不许被改写，
// 否则再打开就没东西可注册了。
test("关掉开关只把行标成禁用，不改写 combo", () => {
  const binding = { combo: "CommandOrControl+Shift+X", enabled: false };
  assert.equal(rowState(binding), "disabled");
  assert.equal(binding.combo, "CommandOrControl+Shift+X");
});

test("有绑定且开关打开时是生效态", () => {
  assert.equal(
    rowState({ combo: "CommandOrControl+Shift+X", enabled: true }),
    "armed",
  );
});

// 空 combo（未设置）和「设置了但关掉」是两回事：前者显示未设置，后者显示绑定。
test("没设快捷键的行一律按未设置处理，与开关无关", () => {
  assert.equal(rowState({ combo: "", enabled: true }), "unset");
  assert.equal(rowState({ combo: null, enabled: true }), "unset");
  assert.equal(rowState({ combo: "", enabled: false }), "unset");
});

// 禁用态要在行上看得出来：绑定还在，只是置灰；开关本身照实跟着标记走。
test("只有停用的绑定才置灰，开关类名照实反映标记", () => {
  assert.deepEqual(
    rowClasses({ combo: "CommandOrControl+Shift+X", enabled: true }),
    { toggle: "on", binding: "" },
  );
  assert.deepEqual(
    rowClasses({ combo: "CommandOrControl+Shift+X", enabled: false }),
    { toggle: "", binding: "shortcut-off" },
  );
});

test("未设置时不置灰，开关按标记显示", () => {
  assert.deepEqual(rowClasses({ combo: null, enabled: true }), { toggle: "on", binding: "" });
  assert.deepEqual(rowClasses({ combo: "", enabled: false }), { toggle: "", binding: "" });
});

test("点开关不进入录制态，点绑定本身才进入", () => {
  const toggle = { closest: sel => (sel === "[data-hotkey-enable]" ? toggle : null) };
  const binding = { closest: () => null };
  assert.equal(shouldEnterRecording({ target: toggle }), false);
  assert.equal(shouldEnterRecording({ target: binding }), true);
});

test("点到开关的子节点同样不算录制", () => {
  const toggle = { closest: () => null };
  const childOfToggle = { closest: sel => (sel === "[data-hotkey-enable]" ? toggle : null) };
  assert.equal(shouldEnterRecording({ target: childOfToggle }), false);
});

// 关掉开关之后重录：新 combo 落盘的同时开关必须回到 on，否则会留下「存了但永不
// 生效」的死状态 —— 报告人正是那个「先取消、哪天又想用回去」的人。
test("禁用态录新 combo 会重新启用并写入新值", () => {
  const parked = { combo: "CommandOrControl+Shift+X", enabled: false };
  assert.equal(rowState(parked), "disabled");
  assert.equal(parked.combo, "CommandOrControl+Shift+X");

  const recorded = recordedBinding("CommandOrControl+Alt+K");
  assert.deepEqual(recorded, { combo: "CommandOrControl+Alt+K", enabled: true });
  assert.equal(rowState(recorded), "armed");
  assert.equal(rowClasses(recorded).binding, "");
});

// 开关那条路只动 enabled：连着关、开、关，combo 字符串一个字都不许变。
test("开关来回拨，combo 字符串一字不变", () => {
  const binding = { combo: "CommandOrControl+Shift+C", enabled: true };
  for (const enabled of [false, true, false]) {
    rowClasses({ combo: binding.combo, enabled });
    assert.equal(binding.combo, "CommandOrControl+Shift+C");
  }
  assert.equal(rowState({ ...binding, enabled: true }), "armed");
});
