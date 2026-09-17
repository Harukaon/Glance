// 版本号比较。Release tag 形如 `v0.2.30`，也可能带后缀（`0.2.31-beta.1`、`0.2.31+build.5`）。
// 「有没有新版本」这件事放在前端判：Rust 的 check_update 只把 GitHub 上的最新 tag
// 与当前版本原样带回来，判定逻辑在这里，能用 node --test 单独测。
//
// 按 SemVer 2.0 的优先级规则来（https://semver.org/）：
//   1. 数字段逐段按整数比，不是字典序（`0.2.10` 比 `0.2.9` 新）；
//   2. 带预发布号的比同号正式版旧（`0.2.31-beta` < `0.2.31`）；
//   3. 预发布号按 `.` 拆段逐段比：纯数字段按整数比（`beta.10` 比 `beta.2` 新），
//      数字段排在字母段之前，逐段相等时段数少的算更旧；
//   4. `+build` 是构建元数据，不参与优先级（`0.2.31+build.5` 与 `0.2.31` 视为同一版）。
// 比 SemVer 松的地方只有一处：允许 `v` 前缀，且数字段可以多于或少于 3 段（缺的按 0 补），
// 因为包里 version 字段历史上出现过 `0.2` 这种写法，别把它判成解析失败。

const VERSION_RE = /^v?(\d+(?:\.\d+)*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$/;

export function parseVersion(raw) {
  const text = typeof raw === "string" ? raw.trim() : "";
  const match = VERSION_RE.exec(text);
  if (!match) return null;
  return {
    numbers: match[1].split(".").map(n => Number.parseInt(n, 10)),
    prerelease: match[2] ? match[2].split(".") : [],
    build: match[3] || "",
  };
}

// 预发布的单段比较：纯数字段按整数比，且永远排在字母段前面。
function comparePrereleasePart(a, b) {
  const aNumeric = /^\d+$/.test(a);
  const bNumeric = /^\d+$/.test(b);
  if (aNumeric && bNumeric) return Number(a) - Number(b);
  if (aNumeric) return -1;
  if (bNumeric) return 1;
  if (a === b) return 0;
  return a < b ? -1 : 1;
}

function comparePrerelease(a, b) {
  // 没有预发布号的那一边更新（0.2.31 > 0.2.31-beta）。
  if (a.length === 0 || b.length === 0) {
    if (a.length === b.length) return 0;
    return a.length === 0 ? 1 : -1;
  }
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    if (a[i] === undefined) return -1;
    if (b[i] === undefined) return 1;
    const diff = comparePrereleasePart(a[i], b[i]);
    if (diff !== 0) return diff;
  }
  return 0;
}

// 返回 >0 / 0 / <0，读作「a 比 b 更新 / 同级 / 更旧」。
// 有一边解析不出来就按 0 处理，调用方据此不会报「有新版本」。
export function compareVersions(a, b) {
  const va = parseVersion(a);
  const vb = parseVersion(b);
  if (!va || !vb) return 0;
  const len = Math.max(va.numbers.length, vb.numbers.length);
  for (let i = 0; i < len; i++) {
    const x = va.numbers[i] || 0;
    const y = vb.numbers[i] || 0;
    if (x !== y) return x - y;
  }
  return comparePrerelease(va.prerelease, vb.prerelease);
}

export function isNewerVersion(latest, current) {
  return compareVersions(latest, current) > 0;
}
