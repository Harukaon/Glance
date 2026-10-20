import test from "node:test";
import assert from "node:assert/strict";

import { requiredHeight, nextHeight, grewAfterResize } from "./window-resize.mjs";

// The failure these pin down (introduced by dfc4aae): with the panel open,
// `.bento-app.settings-open .text-block` is `flex: 1 1 auto`, i.e. the only grow:1
// child, so its offsetHeight = natural height + all the leftover window height.
// settingsHeight() added that as a natural height, so the sum equalled the current
// inner height and `Math.ceil(total)+2` grew the window by 2px on every call (#27).
// requiredHeight() must therefore be blind to how tall the window currently is.

const BASE = { headerH: 56, textBlockMinH: 200, gap: 12, padding: 24 };
const required = sectionsH => requiredHeight({ ...BASE, sectionsH });

test("requiredHeight 对 grow:1 自依赖免疫：喂不同的 text-block 当前实测高，输出不变", () => {
  const base = { ...BASE, sectionsH: 300 };
  const fitted = requiredHeight(base);
  // textBlockH 就是 offsetHeight —— 窗口越高它越大，因为 flex-grow 吃掉全部富余。
  assert.equal(requiredHeight({ ...base, textBlockH: 200 }), fitted);
  assert.equal(requiredHeight({ ...base, textBlockH: 123456 }), fitted);
  // 任何「当前窗口多高」类的入参都不参与计算。
  assert.equal(
    requiredHeight({ ...base, textBlockH: 1e6, currentH: 1e6, windowH: 1e6, innerHeight: 1e6 }),
    fitted,
  );
  // 只有 CSS 下限参与计算，且没有 +2 这类补偿项。
  assert.equal(fitted, 56 + 200 + 300 + 12 * 2 + 24);
});

// #27 的主判据：面板开着点引擎按钮（切走再切回来也一样），只要内容本来就装得下，
// 就不许再 set_size —— 改前的代码是无条件 invoke 一次。
test("面板开着切引擎，内容装得下时不动窗口", () => {
  const needed = required(300);
  assert.equal(needed > 0, true);
  assert.equal(
    nextHeight({ currentH: needed + 400, requiredH: needed, panelOpen: true, grew: 200 }),
    null,
  );
  // 刚好装得下也不算「装不下」。
  assert.equal(
    nextHeight({ currentH: needed, requiredH: needed, panelOpen: true, grew: 200 }),
    null,
  );
});

// 「点当前已选中的引擎」也走完整路径（引擎按钮不与旧值比较，app.js 里那条赋值没有
// 前置判断），所以这条量的是零次 set_size，不是「set 成同样的值」。
test("点当前已选中的引擎按钮，一次 set_size 都不发", () => {
  const remembered = 400; // 折叠态的窗口高，面板打开必然要撑高一次
  const needed = required(300);
  // 打开面板那一次是必要的增高，grew 由纯函数记账。
  let grew = grewAfterResize({ currentH: remembered, targetH: needed, previousGrew: 0 });
  assert.equal(grew, needed - remembered);
  let setSizes = 0;
  for (let click = 0; click < 5; click++) {
    const next = nextHeight({
      currentH: needed,
      requiredH: required(300),
      panelOpen: true,
      rememberedH: remembered,
      grew,
    });
    if (next !== null) {
      setSizes++;
      grew = grewAfterResize({ currentH: needed, targetH: next, previousGrew: grew });
    }
  }
  assert.equal(setSizes, 0);
});

// 棘轮判据：内容变高（切到 LLM 引擎）允许涨一次，切回来不许缩，再切过去也不许再涨。
test("来回切引擎，除了必要的那一次之外零增长", () => {
  const plain = required(300);
  const llm = required(500);
  assert.equal(llm > plain, true);

  let current = plain;
  let grew = 0;
  let grows = 0;
  const click = sectionsH => {
    const next = nextHeight({ currentH: current, requiredH: required(sectionsH), panelOpen: true, grew });
    if (next !== null) {
      if (next > current) grows++;
      grew = grewAfterResize({ currentH: current, targetH: next, previousGrew: grew });
      current = next;
    }
  };
  click(500); // 切到 LLM：装不下，允许增高一次
  click(300); // 切回普通：内容变少，一律不缩窗
  click(500); // 再切回 LLM：已经装得下，必须不动
  click(300);
  assert.equal(grows, 1);
  assert.equal(current, llm);
});

// #23 的字面要求：没手动拖过时，关面板回到打开那一刻的高度，而不是写死的 400。
test("没手动拖时，关面板回到打开前那一刻，不是 400", () => {
  const remembered = 900;
  const target = remembered + 200; // 开面板时我们替他加高
  const grew = grewAfterResize({ currentH: remembered, targetH: target, previousGrew: 0 });
  assert.equal(grew, 200);
  assert.equal(nextHeight({ currentH: target, panelOpen: false, grew }), 900);
  assert.notEqual(nextHeight({ currentH: target, panelOpen: false, grew }), 400);
  // 面板压根没动过窗口（grew 为 0）时，关面板也一律不动。
  assert.equal(nextHeight({ currentH: 900, panelOpen: false, grew: 0 }), null);
});

// 报告人的真实动作：他就是为了看完整译文才把窗口拖大的。
test("面板开着时用户手动拖高 300，关面板回到拖后的值，不是打开前的快照", () => {
  const remembered = 900;
  const grew = grewAfterResize({ currentH: remembered, targetH: 1100, previousGrew: 0 });
  const dragged = 1100 + 300; // 面板开着，用户又拖高 300
  assert.equal(nextHeight({ currentH: dragged, panelOpen: false, grew }), dragged - 200);
  assert.notEqual(nextHeight({ currentH: dragged, panelOpen: false, grew }), remembered);
});

// 拖高之后又发生第二次增高：grew 只累计我们自己垫的那两次，不许把他的拖高算进来。
// 这条就是 `lastTarget - 打开前那一刻` 那种记法会出错的地方（它会还出 900）。
test("拖高之后再增高，关面板仍保住用户拖的那部分", () => {
  const remembered = 900;
  let grew = grewAfterResize({ currentH: remembered, targetH: 1100, previousGrew: 0 });
  let current = 1100 + 50; // 用户拖高 50
  grew = grewAfterResize({ currentH: current, targetH: 1300, previousGrew: grew }); // 切 LLM 又要高
  current = 1300;
  assert.equal(grew, 200 + 150);
  assert.equal(nextHeight({ currentH: current, panelOpen: false, grew }), 950); // 900 + 他那 50
});

// 还高度不许把窗口压到打开前那一刻以下（用户在面板开着时往小拖的情况）。
test("关面板还给的高度不低于打开前那一刻", () => {
  const remembered = 1000;
  const grew = grewAfterResize({ currentH: 1000, targetH: 1200, previousGrew: 200 });
  // 面板开着时用户把窗口往小拖到 1100，欠他的仍是 400，但 1100-400 已经低于起点。
  assert.equal(grew, 400);
  assert.equal(nextHeight({ currentH: 1100, panelOpen: false, rememberedH: remembered, grew }), 1000);
});

// 还高度不许把窗口压到 0 以下，读数本身不可用时也不许乱动。
test("关面板的结果不成立就不动窗口", () => {
  assert.equal(nextHeight({ currentH: 150, panelOpen: false, rememberedH: 0, grew: 400 }), null);
  assert.equal(nextHeight({ currentH: 0, panelOpen: false, rememberedH: 0, grew: 200 }), null);
  assert.equal(nextHeight({ currentH: undefined, panelOpen: false, rememberedH: 0, grew: 200 }), null);
});

// grew 只增不减：没实际增高时不许把它算小或算大。
test("grewAfterResize 只累计实际发生的增高", () => {
  assert.equal(grewAfterResize({ currentH: 900, targetH: 1100, previousGrew: 0 }), 200);
  assert.equal(grewAfterResize({ currentH: 1100, targetH: 1100, previousGrew: 200 }), 200);
  assert.equal(grewAfterResize({ currentH: 1500, targetH: 1100, previousGrew: 200 }), 200);
  assert.equal(grewAfterResize({ currentH: NaN, targetH: 1100, previousGrew: 200 }), 200);
  assert.equal(grewAfterResize({}), 0);
});

// 面板开着但内容确实长高了时仍要能撑开，否则设置项会被裁掉。
test("面板开着且内容装不下时才增高", () => {
  const needed = required(500);
  assert.equal(nextHeight({ currentH: 400, requiredH: needed, panelOpen: true, grew: 0 }), needed);
});

// set_size(LogicalSize) 在 1.5x 下落点带符号不定的误差（请求 402 落 403.3、请求 550
// 落 549.3）。若把 1px 级的差也照发，窗口会在每个「开→关」循环里稳定长一点 —— 与
// #27 同形，只是驱动从点击换成开合。所以差值在这条容差以内就不发 set_size。
test("关面板时 1px 级落点误差不触发 set_size（防每个开合循环涨一点）", () => {
  const grew = grewAfterResize({ currentH: 400, targetH: 550, previousGrew: 0 });
  assert.equal(grew, 150);
  // 请求 400 实际落在 401.3：差 1.3px，什么都不做，停在原地。
  assert.equal(nextHeight({ currentH: 401.3, panelOpen: false, rememberedH: 400, grew }), null);
  assert.equal(nextHeight({ currentH: 402, panelOpen: false, rememberedH: 400, grew }), null);
  // 差 2px 是容差边界，仍不动。
  assert.equal(nextHeight({ currentH: 402, panelOpen: false, rememberedH: 400, grew }), null);
});

test("关面板时超过容差的差照发（用户手拖的那部分要还）", () => {
  const grew = grewAfterResize({ currentH: 400, targetH: 550, previousGrew: 0 });
  // 差 3px 以上已经超出落点误差量级，属于真实尺寸差，必须发。
  assert.equal(nextHeight({ currentH: 403, panelOpen: false, rememberedH: 400, grew }), 400);
  assert.equal(nextHeight({ currentH: 1500, panelOpen: false, rememberedH: 400, grew }), 1350);
});

// 真机读数（21:30 那版）：连续三次「开设置→关设置」后关的落点是 401.3 → 403.3 →
// 405.3，每个循环涨 2px。机制是两个方向的 1px 偏置叠起来：请求 549 实际落 824 物理、
// 读回来是 550（innerHeight = ceil(phys/1.5)），于是 grew 记 149 而实际加了 150，关时
// target = 550 - 149 = 401，再被 ceil 读回 402。所以判定不能比 |target - current|，要
// 比「该还的量」与「开前记下的高度」——两者只差 1px 时答案就是开前那个值本身。
const landing = req => Math.ceil(Math.round(req * 1.5) / 1.5); // 1.5x 下的落点读数

function openCloseCycle(innerH, requiredH = 549) {
  const rememberedH = innerH;
  const grew = grewAfterResize({ currentH: innerH, targetH: requiredH, previousGrew: 0 });
  // 装得下：一次 set_size 都不发，窗口一动不动，关面板也不该动它。
  if (grew === 0) return innerH;
  const opened = landing(requiredH);
  const target = nextHeight({ currentH: opened, panelOpen: false, rememberedH, grew });
  return target === null ? opened : landing(target);
}

test("三次「开→关」是幂等的，不再每循环涨 2px", () => {
  const first = openCloseCycle(400);
  const second = openCloseCycle(first);
  const third = openCloseCycle(second);
  assert.equal(first, 400);
  assert.equal(second, 400);
  assert.equal(third, 400);
});

test("用户拖高之后的开合仍保住他拖的部分", () => {
  // 900 装得下 549 的内容，一次 set_size 都不发，关面板也不动。
  assert.equal(openCloseCycle(900), 900);
});
