import { focusTextInputIfAllowed } from "./focus-helpers.mjs";

const LANGUAGES = [
  { value: "auto", label: "自动检测" },
  { value: "zh-CHS", label: "中文简体" },
  { value: "zh-CHT", label: "中文繁体" },
  { value: "en", label: "英语" },
  { value: "ja", label: "日语" },
  { value: "ko", label: "韩语" },
  { value: "fr", label: "法语" },
  { value: "de", label: "德语" },
  { value: "ru", label: "俄语" },
  { value: "es", label: "西班牙语" }
];

const TTS_LANG_MAP = {
  "zh-CHS": "zh-CN", "zh-CN": "zh-CN",
  "zh-CHT": "zh-TW", "zh-TW": "zh-TW",
  "en": "en-US", "ja": "ja-JP", "ko": "ko-KR",
  "fr": "fr-FR", "de": "de-DE", "ru": "ru-RU", "es": "es-ES"
};

const TRANSLATE_ENGINES = [
  { value: "bing", label: "必应" },
  { value: "google", label: "Google" },
  { value: "microsoft", label: "微软" },
  { value: "transmart", label: "腾讯" },
  { value: "yandex", label: "Yandex" },
  { value: "iciba", label: "词霸" },
  { value: "llm", label: "AI 大模型" }
];

const PROXY_MODES = [
  { value: "system", label: "系统代理" },
  { value: "custom", label: "自定义" },
  { value: "none", label: "不使用" }
];

// LLM prompt presets — 两个可组合维度：风格（通用） + 行业。
// 选择后生成指定源语言 / 自动检测源语言两套提示词并覆盖保存。
const LLM_STYLE_PRESETS = [
  { value: "general", label: "通用翻译", clause: "" },
  { value: "concise", label: "简洁翻译", clause: "Be concise: keep the meaning faithful." },
  { value: "casual", label: "口语化", clause: "Translate into natural, colloquial everyday language, keeping the tone casual and idiomatic." },
  { value: "formal", label: "正式书面", clause: "Use formal written style with precise, standard vocabulary." }
];

const LLM_INDUSTRY_PRESETS = [
  { value: "none", label: "不限行业", role: "translator", clause: "" },
  { value: "tech", label: "IT 技术", role: "translator specializing in IT", clause: "Keep technical terms accurate and consistent with common industry usage (e.g. keep terms like API, SDK, JSON where appropriate)." },
  { value: "legal", label: "法律", role: "legal translator", clause: "Use precise legal terminology and keep the sentence structure faithful to the legal register." },
  { value: "medical", label: "医学", role: "medical translator", clause: "Use accurate medical terminology (anatomy, drug and clinical terms) and keep dosage and numbers exact." },
  { value: "finance", label: "金融", role: "financial translator", clause: "Use standard financial and accounting terminology. Keep numbers, currency, percentages and dates exact." },
  { value: "academic", label: "学术论文", role: "academic translator", clause: "Maintain the academic register, precise argumentation, and consistent technical terms and citations." },
  { value: "business", label: "商务邮件", role: "business communication translator", clause: "Translate into professional, courteous business correspondence with a clear and polite tone." },
  { value: "marketing", label: "市场营销", role: "marketing copy translator", clause: "Translate into natural, engaging marketing copy, adapting idioms and cultural references while preserving the brand voice." }
];

const PRESET_CUSTOM = "custom";

function buildPresetPrompts(styleVal, industryVal) {
  const style = LLM_STYLE_PRESETS.find(s => s.value === styleVal) || LLM_STYLE_PRESETS[0];
  const ind = LLM_INDUSTRY_PRESETS.find(i => i.value === industryVal) || LLM_INDUSTRY_PRESETS[0];
  const extra = [ind.clause, style.clause].filter(Boolean).join(" ");
  const tail = extra ? ` ${extra} Output only the translation, nothing else. Do not add explanations or notes.` : " Output only the translation, nothing else. Do not add explanations or notes.";
  return {
    prompt: `You are a professional ${ind.role}. Translate the following text from {from} to {to}.${tail}`,
    autoPrompt: `You are a professional ${ind.role}. Detect the source language and translate the following text to {to}.${tail}`
  };
}

// Find which (style, industry) combo produced the current prompts, or null.
function presetComboFor(prompt, autoPrompt) {
  const p = (prompt || "").trim();
  const a = (autoPrompt || "").trim();
  for (const s of LLM_STYLE_PRESETS) {
    for (const i of LLM_INDUSTRY_PRESETS) {
      const built = buildPresetPrompts(s.value, i.value);
      if (built.prompt === p && built.autoPrompt === a) return { style: s.value, industry: i.value };
    }
  }
  return null;
}

function presetSelectOptions(list, current, withCustom) {
  return list.map(x =>
    `<option value="${x.value}" ${x.value === current ? "selected" : ""}>${escapeHtml(x.label)}</option>`
  ).join("") + (withCustom ? `<option value="${PRESET_CUSTOM}" ${current === PRESET_CUSTOM ? "selected" : ""}>自定义</option>` : "");
}

// Auto-expand the advanced prompt editor when the user is already on custom
// prompts (so their customizations stay discoverable).
function llmAdvancedExpanded() {
  const cfg = state.settings?.llmConfig;
  if (!cfg) return false;
  return presetComboFor(cfg.prompt, cfg.autoPrompt) === null;
}

// Current combo (or "custom") for the two preset selects.
function presetComboCurrent() {
  const cfg = state.settings?.llmConfig;
  if (!cfg) return { style: PRESET_CUSTOM, industry: PRESET_CUSTOM };
  const combo = presetComboFor(cfg.prompt, cfg.autoPrompt);
  return combo || { style: PRESET_CUSTOM, industry: PRESET_CUSTOM };
}

// Measure the height the window needs to fit the whole app content (header +
// translation area + settings panel) without leaving blank space.
//
// IMPORTANT: the settings panel uses `flex: 1 1 auto`, so once the window has
// been enlarged the panel is stretched to fill the remaining space and its
// `offsetHeight`/`scrollHeight` no longer reflect the content's natural height.
// Reading those would make the window grow a little on every engine switch.
// Instead we measure the panel's *content* (its `.settings-section` children),
// which is unaffected by how tall the panel is stretched.
function settingsHeight() {
  const appEl = document.querySelector(".bento-app");
  const header = document.querySelector(".header-block");
  const textBlock = document.querySelector(".text-block");
  const panel = document.querySelector("#settings-panel");
  if (appEl && header && textBlock && panel) {
    const appCs = getComputedStyle(appEl);
    const appPadding = parseFloat(appCs.paddingTop) + parseFloat(appCs.paddingBottom);
    const appGap = parseFloat(appCs.rowGap || appCs.gap) || 0;

    // Natural content height of the settings panel = its own vertical padding
    // plus the height of every visible section inside it.
    const panelCs = getComputedStyle(panel);
    let panelContent = parseFloat(panelCs.paddingTop) + parseFloat(panelCs.paddingBottom);
    panel.querySelectorAll(":scope > .settings-section").forEach(section => {
      if (getComputedStyle(section).display === "none") return;
      const sCs = getComputedStyle(section);
      panelContent +=
        section.offsetHeight +
        parseFloat(sCs.marginTop) +
        parseFloat(sCs.marginBottom);
    });

    // header + text-block + panel content, with two gaps between the three
    // top-level blocks, plus the app container padding.
    const total =
      header.offsetHeight +
      textBlock.offsetHeight +
      panelContent +
      appGap * 2 +
      appPadding;
    const measured = Math.ceil(total) + 2;
    if (measured > 0) return measured;
  }
  // Fallback estimate.
  let h = 560;
  if (state.settings?.textTranslateEngine === "llm") h += 200;
  if (state.settings?.proxyMode === "custom") h += 56;
  return h;
}

let debounceTimer = null;
// Monotonic id for translation requests. Each new request supersedes older
// in-flight ones so their (stale) results are discarded, letting the user edit
// and re-translate at any time — even mid-translation.
let translateSeq = 0;

function debouncedTranslate() {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    const source = state.activeSide === "right" ? state.rightText : state.leftText;
    const text = (source || "").trim();
    if (text) {
      translateText(state.activeSide === "right" ? "right" : "left");
    } else {
      // Source cleared: cancel any in-flight result and reset the output side.
      translateSeq++;
      state.textLoading = false;
      state.alternatives = [];
      state.detectedLang = "";
      if (state.activeSide === "right") state.leftText = "";
      else state.rightText = "";
      updateSides();
    }
  }, 500);
}

const app = document.querySelector("#app");
const mode = window.__APP_MODE__ || "main";
document.body.dataset.mode = mode.startsWith("overlay") ? "overlay" : "main";

let invoke, listen;

const state = {
  settings: null,
  overlay: null,
  status: "",
  statusType: "",
  loading: false,
  listenersBound: false,
  leftText: "",
  rightText: "",
  activeSide: "left",
  alternatives: [],
  textLoading: false,
  detectedLang: "",
  ttsPlaying: false,
  hotkeyRecording: false,
  settingsOpen: false,
  textHistoryCache: []
};

function defaultSettings() {
  return {
    fromLang: "auto",
    toLang: "zh-CHS",
    captureToLang: "zh-CHS",
    clientele: "deskdict",
    client: "deskdict",
    vendor: "fanyiweb_navigation",
    inputChannel: "YoudaoDict_fanyiweb_navigation",
    appVersion: "10.3.0",
    abTest: "2",
    model: "default",
    screen: "1920*1080",
    osVersion: "14.0",
    network: "none",
    mid: "macos14.0",
    product: "macdict",
    yduuid: `web-${Date.now()}`,
    overlayOpacity: 0.92,
    overlayFontScale: 1,
    closeOnOutsideClick: true,
    autostart: false,
    hotkey: "CommandOrControl+Shift+X",
    copyHotkey: "CommandOrControl+Shift+C",
    textTranslateEngine: "bing",
    llmConfig: {
      baseUrl: "https://api.openai.com/v1/chat/completions",
      apiKey: "",
      model: "gpt-4o-mini",
      prompt: "You are a professional translator. Translate the following text from {from} to {to}. Only output the translation, nothing else. Do not add explanations or notes.",
      autoPrompt: "You are a professional translator. Detect the source language and translate the following text to {to}. Only output the translation, nothing else. Do not add explanations or notes.",
      maxTokens: 4096
    },
    popupShortcut: null,
    proxyMode: "system",
    customProxy: "",
    historyLimit: 200,
    cacheSize: 200
  };
}

/* ── Helpers ── */

function escapeHtml(v) {
  return String(v).replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;")
    .replaceAll('"',"&quot;").replaceAll("'","&#39;");
}

// 粘贴：严格按“粘到哪一侧，哪一侧就是源”的语义处理，不做方向猜测。
// 方向规则：左侧输入 → 左侧为源，右侧输出 {toLang} 译文；
//           右侧输入 → 右侧为源，左侧输出 {fromLang} 译文。
// 语言错配/混合文本由 LLM 提示词硬规则兜底，不再做前端方向纠正。
function handlePaste(side, value) {
  if (!value) return; // 空值不处理，避免覆盖已有文本
  if (side === "left") {
    state.leftText = value;
    state.activeSide = "left";
  } else {
    state.rightText = value;
    state.activeSide = "right";
  }
  translateText(side);
}

function delay(ms) { return new Promise(r => setTimeout(r, ms)); }

function waitForNextPaint() {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(resolve);
    });
  });
}

function setCapturePreparing(active) {
  document.documentElement.classList.toggle("capture-preparing", active);
  document.body.classList.toggle("capture-preparing", active);
}

function languageOptions(current, autoDisabled, hideAuto) {
  return LANGUAGES.filter(l => !(hideAuto && l.value === "auto")).map(l =>
    `<option value="${l.value}" ${l.value === current ? "selected" : ""}${l.value === "auto" && autoDisabled ? " disabled" : ""}>${l.label}</option>`
  ).join("");
}

// Re-render the two language dropdowns preserving their current values. The
// "自动检测" option of one side is disabled when the other side is set to it,
// since both sides can't be "auto" at the same time.
function refreshLangSelects() {
  const from = document.querySelector("#from-lang");
  const to = document.querySelector("#to-lang");
  if (from) from.innerHTML = languageOptions(from.value, state.settings?.toLang === "auto");
  if (to) to.innerHTML = languageOptions(to.value, state.settings?.fromLang === "auto");
}

function shortcutKeysHtml(hk) {
  const parts = hk.replace("CommandOrControl", "Ctrl").split("+");
  return parts.map(p => `<span class="shortcut-key">${escapeHtml(p)}</span>`).join(" + ");
}

function renderFatal(msg) {
  if (app) app.innerHTML = `<div class="bento-app"><div class="block" style="padding:20px"><span class="status-text error">启动失败: ${escapeHtml(msg)}</span></div></div>`;
}

/* ── Tauri bootstrap ── */

async function ensureTauriApi() {
  if (invoke && listen) return;
  for (let i = 0; i < 100; i++) {
    const t = window.__TAURI__;
    const ti = window.__TAURI_INTERNALS__;
    const nextInvoke = t?.core?.invoke || ti?.invoke;
    const nextListen = t?.event?.listen || ti?.event?.listen;
    if (nextInvoke) {
      invoke = nextInvoke;
      listen = nextListen || null;
      return;
    }
    await delay(20);
  }
  throw new Error("Tauri runtime unavailable");
}

async function loadSettings() { state.settings = await invoke("load_settings"); }
async function saveSettings() { state.settings = await invoke("save_settings", { settings: state.settings }); }

async function bindMainListeners() {
  if (state.listenersBound || mode !== "main") return;
  if (!listen) {
    state.listenersBound = true;
    return;
  }
  await listen("workflow:state", (event) => {
    const p = event.payload || {};
    state.loading = Boolean(p.busy);
    if (typeof p.message === "string") { state.status = p.message; state.statusType = p.type || ""; }
    if (!p.busy && (!p.message || p.type === "error")) {
      setCapturePreparing(false);
    }
    updateSides();
  });
  await listen("main:focus-text-input", () => {
    focusTextInputIfAllowed({
      mode,
      hotkeyRecording: state.hotkeyRecording,
      input: document.querySelector(state.activeSide === "right" ? "#text-input-right" : "#text-input"),
    });
  });
  state.listenersBound = true;
}

/* ── Main view render ── */

function renderMain() {
  if (!state.settings) {
    app.innerHTML = `<div class="bento-app"><div class="block" style="padding:20px"><span class="status-text">正在加载…</span></div></div>`;
    return;
  }

  app.innerHTML = `
    <div class="bento-app${state.settingsOpen ? " settings-open" : ""}">
      <div class="block header-block" data-tauri-drag-region>
        <div class="header-left">
          <div class="app-title">Glance</div>
          <div class="lang-pill">
            <select id="from-lang">${languageOptions(state.settings.fromLang, state.settings.toLang === "auto")}</select>
            <span class="lang-icon">➔</span>
            <select id="to-lang">${languageOptions(state.settings.toLang, state.settings.fromLang === "auto")}</select>
          </div>
        </div>
        <div class="header-right">
          <button class="capture-btn" id="capture-btn" title="截图翻译">⛶ 截图翻译</button>
          <div class="lang-pill capture-pill" title="截图翻译目标语言">
            <span class="lang-icon">⛶</span>
            <select id="capture-to-lang">${languageOptions(state.settings.captureToLang, true, true)}</select>
          </div>
          <button class="settings-btn" id="settings-btn" title="设置">⚙</button>
        </div>
      </div>

      <div class="block text-block">
        <div class="text-col input-col">
          <div class="input-wrap">
            <textarea class="input-area" id="text-input" placeholder="输入要翻译的文本..." rows="3"></textarea>
          </div>
          <div class="meta-info">
            <span class="detect-tag" id="detected-lang" style="display:none"></span>
            <span class="status-text" id="status-left" style="display:none"></span>
            <span class="output-alternatives" id="alternatives-left" style="display:none"></span>
            <button class="tts-btn" id="tts-btn" title="朗读">🔊</button>
          </div>
        </div>
        <div class="text-col output-col">
          <div class="input-wrap">
            <textarea class="input-area" id="text-input-right" placeholder="输入要翻译的文本..." rows="3"></textarea>
          </div>
          <div class="meta-info">
            <span class="detect-tag" id="detected-lang-right" style="display:none"></span>
            <span class="status-text" id="status-right" style="display:none"></span>
            <span class="output-alternatives" id="alternatives-right" style="display:none"></span>
            <button class="tts-btn" id="tts-btn-right" title="朗读">🔊</button>
          </div>
        </div>
      </div>

      <div class="settings-panel" id="settings-panel" style="${state.settingsOpen ? "" : "display:none"}">
        <div class="settings-section">
          <button class="section-header" id="sec-translate-toggle" type="button">
            <span class="section-arrow">▾</span> 翻译设置
          </button>
          <div class="section-body" id="sec-translate-body">
            <div class="settings-row">
              <span class="settings-label">翻译引擎</span>
              <div class="engine-switcher" id="engine-switcher">
                ${TRANSLATE_ENGINES.map(e =>
                  `<button class="engine-btn ${state.settings.textTranslateEngine === e.value ? "active" : ""}" data-engine="${e.value}">${e.label}</button>`
                ).join("")}
              </div>
            </div>
            <div class="settings-row">
              <span class="settings-label">网络代理</span>
              <div class="engine-switcher" id="proxy-switcher">
                ${PROXY_MODES.map(p =>
                  `<button class="engine-btn ${state.settings.proxyMode === p.value ? "active" : ""}" data-proxy="${p.value}">${p.label}</button>`
                ).join("")}
              </div>
            </div>
            <div class="settings-row" id="custom-proxy-row" style="${state.settings.proxyMode === "custom" ? "" : "display:none"}">
              <span class="settings-label">代理地址</span>
              <input class="settings-input" id="custom-proxy" type="text"
                      value="${escapeHtml(state.settings.customProxy || "")}"
                      placeholder="http://127.0.0.1:7890" />
            </div>
          </div>
        </div>
        <div class="settings-section">
          <button class="section-header" id="sec-general-toggle" type="button">
            <span class="section-arrow">▸</span> 通用设置
          </button>
          <div class="section-body" id="sec-general-body" style="display:none">
            <div class="settings-row">
              <span class="settings-label">开机自启</span>
              <button class="toggle ${state.settings.autostart ? "on" : ""}" id="autostart" aria-pressed="${state.settings.autostart}"></button>
            </div>
            <div class="settings-row">
              <span class="settings-label">截图翻译</span>
              <div class="shortcut-row settings-shortcut" id="shortcut-row">
                快捷键: ${state.settings.hotkey ? shortcutKeysHtml(state.settings.hotkey) : "未设置"} <span class="shortcut-hint">点击可设置</span>
              </div>
            </div>
            <div class="settings-row">
              <span class="settings-label">截图复制</span>
              <div class="shortcut-row settings-shortcut" id="copy-shortcut-row">
                快捷键: ${state.settings.copyHotkey ? shortcutKeysHtml(state.settings.copyHotkey) : "未设置"} <span class="shortcut-hint">点击可设置</span>
              </div>
            </div>
            <div class="settings-row">
              <span class="settings-label">弹出窗口</span>
              <div class="shortcut-row settings-shortcut" id="popup-shortcut-row">
                ${state.settings.popupShortcut ? shortcutKeysHtml(state.settings.popupShortcut) : "未设置"} <span class="shortcut-hint">点击可设置</span>
              </div>
            </div>
          </div>
        </div>
        <div class="settings-section" id="llm-settings" style="${state.settings.textTranslateEngine === "llm" ? "" : "display:none"}">
          <button class="section-header" id="sec-llm-toggle" type="button">
            <span class="section-arrow">▾</span> AI 大模型
          </button>
          <div class="section-body" id="sec-llm-body">
            <div class="settings-row">
              <span class="settings-label">API 地址 (OpenAI)</span>
              <input class="settings-input" id="llm-base-url" type="text"
                      value="${escapeHtml(state.settings.llmConfig.baseUrl)}"
                      placeholder="https://api.openai.com/v1/chat/completions" />
            </div>
          <div class="settings-row">
            <span class="settings-label">API Key</span>
            <input class="settings-input" id="llm-api-key" type="password"
                    value="${escapeHtml(state.settings.llmConfig.apiKey)}"
                    placeholder="sk-..." />
          </div>
          <div class="settings-row">
            <span class="settings-label">模型</span>
            <input class="settings-input" id="llm-model" type="text"
                    value="${escapeHtml(state.settings.llmConfig.model)}"
                    placeholder="gpt-4o-mini" />
          </div>
          <div class="settings-row">
            <span class="settings-label">最大输出 Tokens</span>
            <input class="settings-input" id="llm-max-tokens" type="number" min="1"
                    value="${escapeHtml(state.settings.llmConfig.maxTokens || 4096)}" />
          </div>
          <div class="settings-row">
            <span class="settings-label">提示词预设
              <span class="settings-hint">风格与行业可组合，选择后覆盖下方自定义提示词</span>
            </span>
            <div class="preset-selects">
              <select class="settings-input settings-select" id="llm-style-preset" title="翻译风格">
                ${presetSelectOptions(LLM_STYLE_PRESETS, presetComboCurrent().style, true)}
              </select>
              <select class="settings-input settings-select" id="llm-industry-preset" title="行业术语">
                ${presetSelectOptions(LLM_INDUSTRY_PRESETS, presetComboCurrent().industry, true)}
              </select>
            </div>
          </div>
          <div class="settings-row">
            <span class="settings-label" style="flex:0 0 auto">
              高级选项
              <span class="settings-hint">自定义提示词</span>
            </span>
            <button class="clear-btn" id="llm-advanced-toggle">${llmAdvancedExpanded() ? "收起" : "展开"}</button>
          </div>
          <div class="llm-advanced" id="llm-advanced" style="${llmAdvancedExpanded() ? "" : "display:none"}">
            <div class="settings-row settings-row-vertical">
              <span class="settings-label">提示词（指定源语言）
                <span class="settings-hint">可用 {from} / {to} 表示源/目标语言</span>
              </span>
              <textarea class="settings-input settings-textarea" id="llm-prompt" rows="4"
                      placeholder="${escapeHtml(defaultSettings().llmConfig.prompt)}">${escapeHtml(state.settings.llmConfig.prompt || "")}</textarea>
            </div>
            <div class="settings-row settings-row-vertical">
              <span class="settings-label">提示词（自动检测源语言）
                <span class="settings-hint">源语言为“自动检测”时使用，可用 {to}</span>
              </span>
              <textarea class="settings-input settings-textarea" id="llm-auto-prompt" rows="4"
                      placeholder="${escapeHtml(defaultSettings().llmConfig.autoPrompt)}">${escapeHtml(state.settings.llmConfig.autoPrompt || "")}</textarea>
            </div>
          </div>
        </div>
        <div class="settings-section">
          <button class="section-header" id="sec-storage-toggle" type="button">
            <span class="section-arrow">▸</span> 存储
          </button>
          <div class="section-body" id="sec-storage-body" style="display:none">
            <div class="settings-row">
              <span class="settings-label">历史记录上限
                <span class="settings-hint">建议 200，范围 0–2000（0 = 不记录）</span>
              </span>
              <input class="settings-input" id="history-limit" type="number" min="0" max="2000"
                      placeholder="200"
                      value="${escapeHtml(state.settings.historyLimit ?? 200)}" />
            </div>
            <div class="settings-row">
              <span class="settings-label">翻译缓存上限（条）
                <span class="settings-hint">建议 200，范围 50–1000</span>
              </span>
              <input class="settings-input" id="cache-size" type="number" min="50" max="1000"
                      placeholder="200"
                      value="${escapeHtml(state.settings.cacheSize ?? 200)}" />
            </div>
            <div class="settings-row">
              <span class="settings-label">文本翻译历史</span>
              <div class="history-actions">
                <button class="clear-btn" id="view-history-btn">查看</button>
                <button class="clear-btn" id="clear-history-btn">清空</button>
              </div>
            </div>
            <div class="history-list" id="history-list" style="display:none"></div>
            <div class="settings-row">
              <span class="settings-label">翻译缓存</span>
              <button class="clear-btn" id="clear-cache-btn">清空</button>
            </div>
          </div>
        </div>
      </div>
    </div>`;

  // Restore input
  const inp = document.querySelector("#text-input");
  const inpR = document.querySelector("#text-input-right");

  updateSides();

// Events
  inp.addEventListener("input", e => { state.leftText = e.target.value; state.activeSide = "left"; debouncedTranslate(); });
  inp.addEventListener("keydown", e => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); translateText("left"); }
  });
  inp.addEventListener("paste", e => {
    // 从剪贴板直接取文本，避免与 input 事件处理器的同步清空竞态。
    const pasted = (e.clipboardData && e.clipboardData.getData("text")) || inp.value;
    setTimeout(() => handlePaste("left", pasted), 0);
  });
  inpR.addEventListener("input", e => { state.rightText = e.target.value; state.activeSide = "right"; debouncedTranslate(); });
  inpR.addEventListener("keydown", e => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); translateText("right"); }
  });
  inpR.addEventListener("paste", e => {
    // 从剪贴板直接取文本，避免与 input 事件处理器的同步清空竞态。
    const pasted = (e.clipboardData && e.clipboardData.getData("text")) || inpR.value;
    setTimeout(() => handlePaste("right", pasted), 0);
  });
  document.querySelector("#from-lang").addEventListener("change", e => { state.settings.fromLang = e.target.value; saveSettings().catch(()=>{}); refreshLangSelects(); updateSides(); });
  document.querySelector("#to-lang").addEventListener("change", e => { state.settings.toLang = e.target.value; saveSettings().catch(()=>{}); refreshLangSelects(); updateSides(); });
  document.querySelector("#capture-to-lang").addEventListener("change", e => { state.settings.captureToLang = e.target.value; saveSettings().catch(()=>{}); });

  document.querySelector("#settings-btn").addEventListener("click", e => {
    e.stopPropagation();
    state.settingsOpen = !state.settingsOpen;
    const panel = document.querySelector("#settings-panel");
    const btn = e.currentTarget;
    panel.style.display = state.settingsOpen ? "" : "none";
    document.querySelector(".bento-app")?.classList.toggle("settings-open", state.settingsOpen);
    btn.classList.toggle("open", state.settingsOpen);
    if (state.settingsOpen) {
      invoke?.("resize_main_window", { height: settingsHeight() }).catch(() => {});
    } else {
      invoke?.("resize_main_window", { height: 400 }).catch(() => {});
    }
  });

  document.querySelector("#autostart").addEventListener("click", e => {
    state.settings.autostart = !state.settings.autostart;
    e.currentTarget.classList.toggle("on", state.settings.autostart);
    e.currentTarget.setAttribute("aria-pressed", state.settings.autostart);
    saveSettings().catch(() => {});
  });
  document.querySelector("#tts-btn").addEventListener("click", speakInput);
  document.querySelector("#tts-btn-right").addEventListener("click", speakInput);

  document.querySelector("#capture-btn").addEventListener("click", e => { e.stopPropagation(); startCapture(); });
  document.querySelector("#shortcut-row").addEventListener("click", e => { e.stopPropagation(); startHotkeyRecording(); });
  document.querySelector("#copy-shortcut-row").addEventListener("click", e => { e.stopPropagation(); startCopyHotkeyRecording(); });
  document.querySelector("#popup-shortcut-row").addEventListener("click", e => { e.stopPropagation(); startPopupShortcutRecording(); });

  // Engine switcher
  document.querySelectorAll("#engine-switcher .engine-btn").forEach(btn => {
    btn.addEventListener("click", e => {
      e.stopPropagation();
      const newEngine = e.currentTarget.dataset.engine;
      state.settings.textTranslateEngine = newEngine;
      saveSettings().catch(() => {});
      // Update active state
      document.querySelectorAll("#engine-switcher .engine-btn").forEach(b => b.classList.toggle("active", b.dataset.engine === newEngine));
      // Toggle LLM settings visibility
      const llmSettings = document.querySelector("#llm-settings");
      if (llmSettings) llmSettings.style.display = newEngine === "llm" ? "" : "none";
      // Resize window only while the settings panel is open; otherwise keep the
      // compact translation view unchanged.
      if (state.settingsOpen) {
        invoke?.("resize_main_window", { height: settingsHeight() }).catch(() => {});
      }
    });
  });

  // Proxy mode switcher
  document.querySelectorAll("#proxy-switcher .engine-btn").forEach(btn => {
    btn.addEventListener("click", e => {
      e.stopPropagation();
      const newMode = e.currentTarget.dataset.proxy;
      state.settings.proxyMode = newMode;
      saveSettings().catch(() => {});
      document.querySelectorAll("#proxy-switcher .engine-btn").forEach(b => b.classList.toggle("active", b.dataset.proxy === newMode));
      const customRow = document.querySelector("#custom-proxy-row");
      if (customRow) customRow.style.display = newMode === "custom" ? "" : "none";
      if (state.settingsOpen) {
        invoke?.("resize_main_window", { height: settingsHeight() }).catch(() => {});
      }
    });
  });
  const customProxyInput = document.querySelector("#custom-proxy");
  if (customProxyInput) customProxyInput.addEventListener("change", e => { state.settings.customProxy = e.target.value.trim(); saveSettings().catch(() => {}); });

  // LLM config inputs
  const baseUrlInput = document.querySelector("#llm-base-url");
  const apiKeyInput = document.querySelector("#llm-api-key");
  const modelInput = document.querySelector("#llm-model");
  const promptInput = document.querySelector("#llm-prompt");
  const autoPromptInput = document.querySelector("#llm-auto-prompt");
  if (baseUrlInput) baseUrlInput.addEventListener("change", e => { state.settings.llmConfig.baseUrl = e.target.value.trim(); saveSettings().catch(() => {}); });
  if (apiKeyInput) apiKeyInput.addEventListener("change", e => { state.settings.llmConfig.apiKey = e.target.value.trim(); saveSettings().catch(() => {}); });
  if (modelInput) modelInput.addEventListener("change", e => { state.settings.llmConfig.model = e.target.value.trim(); saveSettings().catch(() => {}); });
  if (promptInput) promptInput.addEventListener("change", e => { state.settings.llmConfig.prompt = e.target.value; syncPresetSelects(); saveSettings().catch(() => {}); });
  if (autoPromptInput) autoPromptInput.addEventListener("change", e => { state.settings.llmConfig.autoPrompt = e.target.value; syncPresetSelects(); saveSettings().catch(() => {}); });

  // Keep both preset selects in sync when the user edits the prompts.
  function syncPresetSelects() {
    const styleSel = document.querySelector("#llm-style-preset");
    const indSel = document.querySelector("#llm-industry-preset");
    const combo = presetComboFor(state.settings.llmConfig.prompt, state.settings.llmConfig.autoPrompt);
    if (styleSel) styleSel.value = combo ? combo.style : PRESET_CUSTOM;
    if (indSel) indSel.value = combo ? combo.industry : PRESET_CUSTOM;
  }

  // Presets: style + industry combine into one prompt pair. Changing either
  // rebuilds the prompts and saves. Selecting "自定义" keeps the textareas.
  function applyPresetCombo(styleVal, industryVal) {
    if (styleVal === PRESET_CUSTOM || industryVal === PRESET_CUSTOM) return;
    const built = buildPresetPrompts(styleVal, industryVal);
    state.settings.llmConfig.prompt = built.prompt;
    state.settings.llmConfig.autoPrompt = built.autoPrompt;
    const p = document.querySelector("#llm-prompt");
    const a = document.querySelector("#llm-auto-prompt");
    if (p) p.value = built.prompt;
    if (a) a.value = built.autoPrompt;
    saveSettings().catch(() => {});
  }

  document.querySelector("#llm-style-preset")?.addEventListener("change", e => {
    const indSel = document.querySelector("#llm-industry-preset");
    applyPresetCombo(e.target.value, indSel ? indSel.value : PRESET_CUSTOM);
  });
  document.querySelector("#llm-industry-preset")?.addEventListener("change", e => {
    const styleSel = document.querySelector("#llm-style-preset");
    applyPresetCombo(styleSel ? styleSel.value : PRESET_CUSTOM, e.target.value);
  });

  // Settings sections: collapse/expand by category.
  document.querySelectorAll(".section-header").forEach(header => {
    header.addEventListener("click", e => {
      e.stopPropagation();
      const body = header.parentElement.querySelector(":scope > .section-body");
      if (!body) return;
      const expanded = body.style.display !== "none";
      body.style.display = expanded ? "none" : "";
      header.querySelector(".section-arrow").textContent = expanded ? "▸" : "▾";
      if (state.settingsOpen) {
        invoke?.("resize_main_window", { height: settingsHeight() }).catch(() => {});
      }
    });
  });

  // Advanced section toggle (custom prompt editor).
  document.querySelector("#llm-advanced-toggle")?.addEventListener("click", e => {
    e.stopPropagation();
    const box = document.querySelector("#llm-advanced");
    const btn = e.currentTarget;
    if (!box) return;
    const expanded = box.style.display !== "none";
    box.style.display = expanded ? "none" : "";
    btn.textContent = expanded ? "展开" : "收起";
    if (state.settingsOpen) {
      invoke?.("resize_main_window", { height: settingsHeight() }).catch(() => {});
    }
  });
  const maxTokensInput = document.querySelector("#llm-max-tokens");
  if (maxTokensInput) maxTokensInput.addEventListener("change", e => {
    const v = parseInt(e.target.value, 10);
    state.settings.llmConfig.maxTokens = Number.isFinite(v) && v > 0 ? v : 4096;
    maxTokensInput.value = state.settings.llmConfig.maxTokens;
    saveSettings().catch(() => {});
  });

  // Storage section: limits + clear buttons
  const historyLimitInput = document.querySelector("#history-limit");
  if (historyLimitInput) historyLimitInput.addEventListener("change", e => {
    let v = parseInt(e.target.value, 10);
    if (!Number.isFinite(v)) v = 200;
    v = Math.min(2000, Math.max(0, v));
    state.settings.historyLimit = v;
    historyLimitInput.value = v;
    saveSettings().catch(() => {});
  });
  const cacheSizeInput = document.querySelector("#cache-size");
  if (cacheSizeInput) cacheSizeInput.addEventListener("change", e => {
    let v = parseInt(e.target.value, 10);
    if (!Number.isFinite(v)) v = 200;
    v = Math.min(1000, Math.max(50, v));
    state.settings.cacheSize = v;
    cacheSizeInput.value = v;
    saveSettings().catch(() => {});
  });
  document.querySelector("#clear-history-btn")?.addEventListener("click", e => {
    e.stopPropagation();
    invoke?.("clear_text_history").then(() => {
      const btn = document.querySelector("#clear-history-btn");
      if (btn) { btn.textContent = "已清空"; setTimeout(() => { if (btn) btn.textContent = "清空"; }, 1200); }
      const list = document.querySelector("#history-list");
      if (list) { list.innerHTML = ""; list.style.display = "none"; }
      const viewBtn = document.querySelector("#view-history-btn");
      if (viewBtn) viewBtn.textContent = "查看";
    }).catch(() => {});
  });
  document.querySelector("#view-history-btn")?.addEventListener("click", e => {
    e.stopPropagation();
    const btn = e.currentTarget;
    const list = document.querySelector("#history-list");
    if (!list) return;
    if (list.style.display !== "none") {
      list.style.display = "none";
      btn.textContent = "查看";
      return;
    }
    invoke?.("list_text_history").then(items => {
      state.textHistoryCache = items || [];
      const visible = state.textHistoryCache.slice(0, 20);
      if (visible.length === 0) {
        list.innerHTML = `<div class="history-empty">暂无历史记录</div>`;
      } else {
        list.innerHTML = visible.map(item => {
          const time = new Date(item.createdAt).toLocaleString();
          const lang = `${item.fromLang} → ${item.toLang}`;
          return `<div class="history-item" data-id="${escapeHtml(item.id)}">
            <div class="history-text"><span class="history-src">${escapeHtml(item.source)}</span><span class="history-arrow">→</span><span class="history-dst">${escapeHtml(item.translated)}</span></div>
            <div class="history-meta">${escapeHtml(time)} · ${escapeHtml(lang)} · ${escapeHtml(item.engine)}</div>
          </div>`;
        }).join("");
      }
      list.style.display = "";
      btn.textContent = "收起";
      if (state.settingsOpen) {
        invoke?.("resize_main_window", { height: settingsHeight() }).catch(() => {});
      }
    }).catch(() => {});
  });
  // Clicking a history entry restores the source → result pair in the text area.
  document.querySelector("#history-list")?.addEventListener("click", e => {
    const itemEl = e.target.closest(".history-item");
    if (!itemEl) return;
    const item = state.textHistoryCache.find(x => x.id === itemEl.dataset.id);
    if (!item) return;
    state.leftText = item.source;
    state.rightText = item.translated;
    state.activeSide = "left";
    updateSides();
  });
  document.querySelector("#clear-cache-btn")?.addEventListener("click", e => {
    e.stopPropagation();
    invoke?.("clear_translate_cache").then(count => {
      const btn = document.querySelector("#clear-cache-btn");
      if (btn) { btn.textContent = `已清空 ${count} 条`; setTimeout(() => { if (btn) btn.textContent = "清空"; }, 1500); }
    }).catch(() => {});
  });

  (state.activeSide === "right" ? inpR : inp).focus();
}

/* ── Partial DOM updates ── */

// Render both text columns: which side is the current source (read/write),
// which side shows the result (read-only when the language rules require it),
// plus the meta rows (detect tag + TTS on the source side, status +
// alternatives on the result side).
function updateSides() {
  const left = document.querySelector("#text-input");
  const right = document.querySelector("#text-input-right");
  if (!left || !right) return;

  left.value = state.leftText;
  right.value = state.rightText;

  const fromLang = state.settings?.fromLang || "auto";
  const toLang = state.settings?.toLang || "zh-CHS";
  // 目标语言为“自动检测”时无法从左→右翻译；源语言为“自动检测”时无法从右→左翻译。
  const leftLocked = toLang === "auto";
  const rightLocked = fromLang === "auto";

  left.readOnly = leftLocked;
  right.readOnly = rightLocked;
  left.placeholder = leftLocked ? "目标语言为“自动检测”，不能在此输入" : "输入要翻译的文本…";
  right.placeholder = rightLocked ? "源语言为“自动检测”，不能在此输入" : "输入要翻译的文本…";

  let sourceSide = state.activeSide;
  if (rightLocked) sourceSide = "left";
  if (leftLocked) sourceSide = "right";
  const outputSide = sourceSide === "left" ? "right" : "left";

  // Detect tag follows the source side.
  const detectL = document.querySelector("#detected-lang");
  const detectR = document.querySelector("#detected-lang-right");
  for (const el of [detectL, detectR]) if (el) el.style.display = "none";
  if (state.detectedLang) {
    const label = LANGUAGES.find(l => l.value === state.detectedLang)?.label || state.detectedLang;
    const el = sourceSide === "left" ? detectL : detectR;
    if (el) {
      el.textContent = `检测: ${label}`;
      el.style.display = "";
    }
  }

  // TTS button follows the source side.
  const ttsL = document.querySelector("#tts-btn");
  const ttsR = document.querySelector("#tts-btn-right");
  for (const el of [ttsL, ttsR]) if (el) el.style.display = "none";
  const tts = sourceSide === "left" ? ttsL : ttsR;
  if (tts) {
    tts.style.display = "";
    tts.classList.toggle("speaking", state.ttsPlaying);
  }

  // Status + alternatives live on the result side.
  updateOutputMeta("#status-left", "#alternatives-left", outputSide === "left");
  updateOutputMeta("#status-right", "#alternatives-right", outputSide === "right");

  const btn = document.querySelector("#capture-btn");
  if (btn) btn.disabled = state.loading;
}

function updateOutputMeta(statusSel, altSel, visible) {
  const statusEl = document.querySelector(statusSel);
  const altEl = document.querySelector(altSel);
  if (!statusEl || !altEl) return;

  if (!visible) {
    statusEl.innerHTML = "";
    statusEl.style.display = "none";
    altEl.innerHTML = "";
    altEl.style.display = "none";
    return;
  }

  if (state.textLoading) {
    statusEl.style.display = "";
    statusEl.innerHTML = `<span class="dot-loading"><span></span><span></span><span></span></span> 翻译中…`;
    altEl.style.display = "none";
    altEl.innerHTML = "";
  } else if (state.status && state.statusType === "error") {
    statusEl.style.display = "";
    statusEl.textContent = state.status;
    statusEl.classList.add("error");
    altEl.style.display = "none";
    altEl.innerHTML = "";
  } else if (state.loading && state.status) {
    statusEl.style.display = "";
    statusEl.textContent = state.status;
    statusEl.classList.remove("error");
    altEl.style.display = "none";
    altEl.innerHTML = "";
  } else {
    statusEl.innerHTML = "";
    statusEl.style.display = "none";
    if (state.alternatives.length > 0) {
      altEl.style.display = "";
      altEl.innerHTML = state.alternatives.map(a => `<span class="alt-tag">${escapeHtml(a)}</span>`).join("");
    } else {
      altEl.style.display = "none";
      altEl.innerHTML = "";
    }
  }
}

/* ── Actions ── */

async function startCapture() {
  if (state.loading) return;
  try {
    state.loading = true;
    await saveSettings();
    setCapturePreparing(true);
    document.activeElement?.blur?.();
    await waitForNextPaint();
    await invoke("begin_capture", { options: { fromLang: state.settings.fromLang, toLang: state.settings.toLang } });
  } catch (err) {
    setCapturePreparing(false);
    state.loading = false;
    state.status = String(err);
    state.statusType = "error";
    updateSides();
  }
}

async function translateText(side) {
  const isLeft = side !== "right";
  const text = (isLeft ? state.leftText : state.rightText).trim();
  if (!text) return;

  // 反向翻译（右侧输入）：源/目标语言互换。
  const fromLang = isLeft ? state.settings.fromLang : state.settings.toLang;
  const toLang = isLeft ? state.settings.toLang : state.settings.fromLang;
  if (toLang === "auto") return;

  // Claim this as the latest request; older in-flight ones become stale.
  const seq = ++translateSeq;

  state.textLoading = true;
  state.status = "";
  state.statusType = "";
  state.alternatives = [];
  state.detectedLang = "";
  updateSides();

  // Validate LLM config
  if (state.settings.textTranslateEngine === "llm" && !state.settings.llmConfig.apiKey) {
    if (seq === translateSeq) {
      state.textLoading = false;
      state.status = "请先在设置中配置 API Key";
      state.statusType = "error";
      updateSides();
    }
    return;
  }

  try {
    const r = await invoke("translate_text", { text, fromLang, toLang });
    if (seq !== translateSeq) return; // superseded by a newer request
    if (isLeft) state.rightText = r.translatedText;
    else state.leftText = r.translatedText;
    state.alternatives = r.alternatives || [];
    state.detectedLang = r.fromLangDetected;
  } catch (err) {
    if (seq !== translateSeq) return; // superseded; ignore stale error
    state.status = String(err);
    state.statusType = "error";
  } finally {
    // Only the latest request controls the loading state / final render.
    if (seq === translateSeq) {
      state.textLoading = false;
      updateSides();
    }
  }
}

function speakInput() {
  const isLeft = state.activeSide !== "right";
  const text = (isLeft ? state.leftText : state.rightText).trim();
  if (!text) return;

  if (window.speechSynthesis.speaking) {
    window.speechSynthesis.cancel();
    state.ttsPlaying = false;
    updateSides();
    return;
  }

  const utt = new SpeechSynthesisUtterance(text);
  const fromLang = state.detectedLang || (isLeft ? state.settings.fromLang : state.settings.toLang);
  utt.lang = TTS_LANG_MAP[fromLang] || fromLang;
  utt.onend = () => { state.ttsPlaying = false; updateSides(); };
  utt.onerror = utt.onend;

  state.ttsPlaying = true;
  updateSides();
  window.speechSynthesis.speak(utt);
}

/* ── Hotkey recorder ── */

const KEY_MAP = {
  " ":"Space","ArrowUp":"Up","ArrowDown":"Down","ArrowLeft":"Left","ArrowRight":"Right",
  "Escape":"Escape","Enter":"Return","Tab":"Tab","Backspace":"Backspace",
  "Delete":"Delete","Insert":"Insert","Home":"Home","End":"End",
  "PageUp":"PageUp","PageDown":"PageDown",
  "F1":"F1","F2":"F2","F3":"F3","F4":"F4","F5":"F5","F6":"F6",
  "F7":"F7","F8":"F8","F9":"F9","F10":"F10","F11":"F11","F12":"F12",
};

function startHotkeyRecording() {
  if (state.hotkeyRecording) return;
  state.hotkeyRecording = true;
  const row = document.querySelector("#shortcut-row");
  if (!row) return;
  row.classList.add("recording");
  row.innerHTML = `按下快捷键…`;

  function onKey(e) {
    e.preventDefault();
    e.stopPropagation();
    const mods = [];
    if (e.ctrlKey) mods.push("CommandOrControl");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    if (e.metaKey) mods.push("Super");
    if (["Control","Alt","Shift","Meta"].includes(e.key)) return;
    if (e.key === "Escape") { finishRecording(null); return; }
    let key = KEY_MAP[e.key] || (e.key.length === 1 ? e.key.toUpperCase() : null);
    if (!key) {
      return;
    }
    finishRecording([...mods, key].join("+"));
  }

  function finishRecording(combo) {
    document.removeEventListener("keydown", onKey, true);
    state.hotkeyRecording = false;
    if (combo) {
      state.settings.hotkey = combo;
      saveSettings().catch(() => {});
    }
    const r = document.querySelector("#shortcut-row");
    if (r) {
      r.classList.remove("recording");
      r.innerHTML = `快捷键: ${shortcutKeysHtml(state.settings.hotkey)} <span class="shortcut-hint">点击可设置快捷键</span>`;
    }
  }

  document.addEventListener("keydown", onKey, true);
}

function startCopyHotkeyRecording() {
  if (state.hotkeyRecording) return;
  state.hotkeyRecording = true;
  const row = document.querySelector("#copy-shortcut-row");
  if (!row) return;
  row.classList.add("recording");
  row.innerHTML = `按下快捷键…`;

  function onKey(e) {
    e.preventDefault();
    e.stopPropagation();
    const mods = [];
    if (e.ctrlKey) mods.push("CommandOrControl");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    if (e.metaKey) mods.push("Super");
    if (["Control","Alt","Shift","Meta"].includes(e.key)) return;
    if (e.key === "Escape") { finishRecording(null); return; }
    let key = KEY_MAP[e.key] || (e.key.length === 1 ? e.key.toUpperCase() : null);
    if (!key) {
      return;
    }
    finishRecording([...mods, key].join("+"));
  }

  function finishRecording(combo) {
    document.removeEventListener("keydown", onKey, true);
    state.hotkeyRecording = false;
    if (combo) {
      state.settings.copyHotkey = combo;
      saveSettings().catch(() => {});
    }
    const r = document.querySelector("#copy-shortcut-row");
    if (r) {
      r.classList.remove("recording");
      r.innerHTML = `快捷键: ${shortcutKeysHtml(state.settings.copyHotkey)} <span class="shortcut-hint">点击可设置</span>`;
    }
  }

  document.addEventListener("keydown", onKey, true);
}

function startPopupShortcutRecording() {
  if (state.hotkeyRecording) return;
  state.hotkeyRecording = true;
  const row = document.querySelector("#popup-shortcut-row");
  if (!row) return;
  row.classList.add("recording");
  row.innerHTML = `按下快捷键…`;

  function onKey(e) {
    e.preventDefault();
    e.stopPropagation();
    const mods = [];
    if (e.ctrlKey) mods.push("CommandOrControl");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    if (e.metaKey) mods.push("Super");
    if (["Control","Alt","Shift","Meta"].includes(e.key)) return;
    if (e.key === "Escape") { finishRecording(null); return; }
    let key = KEY_MAP[e.key] || (e.key.length === 1 ? e.key.toUpperCase() : null);
    if (!key) {
      return;
    }
    finishRecording([...mods, key].join("+"));
  }

  function finishRecording(combo) {
    document.removeEventListener("keydown", onKey, true);
    state.hotkeyRecording = false;
    state.settings.popupShortcut = combo || null;
    saveSettings().catch(() => {});
    const r = document.querySelector("#popup-shortcut-row");
    if (r) {
      r.classList.remove("recording");
      const display = state.settings.popupShortcut ? shortcutKeysHtml(state.settings.popupShortcut) : "未设置";
      r.innerHTML = `${display} <span class="shortcut-hint">点击可设置</span>`;
    }
  }

  document.addEventListener("keydown", onKey, true);
}

/* ── Overlay (unchanged) ── */

async function renderOverlay() {
  app.innerHTML = `<div class="overlay-root"><div id="overlay-stage"></div></div>`;
  window.addEventListener("keydown", e => { if (e.key === "Escape") invoke("close_overlay"); });
  try {
    state.overlay = await invoke("load_overlay_payload");
    if (state.overlay.closeOnOutsideClick) {
      document.querySelector(".overlay-root").addEventListener("click", () => invoke("close_overlay"));
    }
    const s = state.overlay.selection;
    const src = `data:image/jpeg;base64,${state.overlay.renderedImageBase64}`;
    const dpr = window.devicePixelRatio || 1;
    document.querySelector("#overlay-stage").innerHTML = `
      <img class="overlay-image" src="${src}" alt="translated"
           style="left:${s.x/dpr}px;top:${s.y/dpr}px;width:${s.width/dpr}px;height:${s.height/dpr}px;opacity:${state.overlay.overlayOpacity};" />`;
  } catch (err) { renderFatal(`覆盖层初始化失败: ${err}`); }
}

/* ── Boot ── */

document.addEventListener("keydown", e => {
  if (e.key === "Escape" && mode === "main" && !state.hotkeyRecording) {
    invoke?.("hide_window");
  }
});

window.addEventListener("focus", () => {
  if (mode === "main") {
    setCapturePreparing(false);
  }
});

async function boot() {
  if (mode === "main") {
    renderMain();
  } else if (app) {
    app.innerHTML = `<div class="overlay-root"><div class="capture-error-screen">正在初始化…</div></div>`;
  }
  await ensureTauriApi();
  await bindMainListeners();
  if (mode.startsWith("overlay")) { await renderOverlay(); return; }
  try {
    await loadSettings();
    // 两边不能同时为“自动检测”（反向翻译时目标语言需明确）。
    if (state.settings.fromLang === "auto" && state.settings.toLang === "auto") {
      state.settings.toLang = "zh-CHS";
      saveSettings().catch(() => {});
    }
  } catch (err) {
    console.error("load_settings failed, falling back to defaults", err);
    state.settings = defaultSettings();
    state.status = `设置加载失败，已使用默认配置: ${err instanceof Error ? err.message : String(err)}`;
    state.statusType = "error";
  }
  invoke?.("resize_main_window", { height: 400 }).catch(() => {});
  renderMain();
}

window.addEventListener("error", e => renderFatal(e.error?.message || e.message || "unknown"));
window.addEventListener("unhandledrejection", e => renderFatal(e.reason instanceof Error ? e.reason.message : String(e.reason)));
boot().catch(e => renderFatal(e instanceof Error ? e.message : String(e)));
