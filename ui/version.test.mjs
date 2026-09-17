import test from "node:test";
import assert from "node:assert/strict";

import { compareVersions, isNewerVersion, parseVersion } from "./version.mjs";

test("按数字段比，不是按字符串比", () => {
  assert.equal(isNewerVersion("0.2.10", "0.2.9"), true);
  assert.equal(isNewerVersion("0.2.9", "0.2.10"), false);
  assert.equal(isNewerVersion("0.10.0", "0.9.9"), true);
  assert.equal(isNewerVersion("1.0.0", "0.99.99"), true);
});

test("相同版本不算有新版本", () => {
  assert.equal(isNewerVersion("0.2.30", "0.2.30"), false);
  assert.equal(isNewerVersion("v0.2.30", "0.2.30"), false);
  assert.equal(isNewerVersion("0.2", "0.2.0"), false);
  assert.equal(isNewerVersion("0.2.0.0", "0.2"), false);
});

test("带后缀的比同号正式版旧", () => {
  assert.equal(isNewerVersion("0.2.31-beta.1", "0.2.30"), true);
  assert.equal(isNewerVersion("0.2.30", "0.2.30-beta.1"), true);
  assert.equal(isNewerVersion("0.2.30-beta.1", "0.2.30"), false);
  assert.equal(isNewerVersion("0.2.30-beta.2", "0.2.30-beta.1"), true);
});

test("预发布段按整数比，不是字典序", () => {
  // beta.10 比 beta.2 新；字典序会把它判反。
  assert.equal(isNewerVersion("0.2.31-beta.10", "0.2.31-beta.2"), true);
  assert.equal(isNewerVersion("0.2.31-beta.2", "0.2.31-beta.10"), false);
  assert.equal(isNewerVersion("0.2.31-2", "0.2.31-10"), false);
  // 纯数字段永远排在字母段前面。
  assert.equal(isNewerVersion("0.2.31-alpha", "0.2.31-1"), true);
  assert.equal(isNewerVersion("0.2.31-1", "0.2.31-alpha"), false);
  // 逐段相等时段数少的更旧。
  assert.equal(isNewerVersion("0.2.31-beta.1.0", "0.2.31-beta.1"), true);
  assert.equal(isNewerVersion("0.2.31-beta.1", "0.2.31-beta.1.0"), false);
});

test("+build 是构建元数据，不参与优先级", () => {
  assert.equal(isNewerVersion("0.2.31+build.5", "0.2.31"), false);
  assert.equal(isNewerVersion("0.2.31", "0.2.31+build.5"), false);
  assert.equal(isNewerVersion("0.2.31+build.9", "0.2.31+build.2"), false);
  // 只差构建元数据的 tag 不该提示有新版本，但号真的更新了要能报出来。
  assert.equal(isNewerVersion("0.2.32+build.1", "0.2.31+build.9"), true);
  assert.equal(compareVersions("0.2.31+build.5", "0.2.31"), 0);
  // 预发布 + 构建元数据混在一起时，只有预发布参与比较。
  assert.equal(isNewerVersion("0.2.31-alpha+001", "0.2.31-alpha"), false);
  assert.equal(isNewerVersion("0.2.31-alpha+002", "0.2.31-alpha+001"), false);
});

test("解析不出来的一律按没有新版本处理", () => {
  assert.equal(isNewerVersion("", "0.2.30"), false);
  assert.equal(isNewerVersion("latest", "0.2.30"), false);
  assert.equal(isNewerVersion("0.2.31", ""), false);
  assert.equal(isNewerVersion(undefined, undefined), false);
  assert.equal(isNewerVersion("0.2.31-beta!", "0.2.30"), false);
  assert.equal(parseVersion("nightly"), null);
  assert.deepEqual(parseVersion("v0.2.30"), { numbers: [0, 2, 30], prerelease: [], build: "" });
  assert.deepEqual(parseVersion("0.2.31-beta.2+build.7"), {
    numbers: [0, 2, 31], prerelease: ["beta", "2"], build: "build.7",
  });
});
