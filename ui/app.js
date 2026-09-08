import { focusTextInputIfAllowed } from "./focus-helpers.mjs";
import { shouldTranslateOnInput, shouldTranslateOnEnter } from "./ime-guards.mjs";

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

// Measure the height the window needs to fit the whole app content (header +
// translation area + settings panel) without leaving blank space.
//
// IMPORTANT: the settings panel uses `flex: 1 1 auto`, so once the window has
// been enlarged the panel is stretched to fill the remaining space and its
// `offsetHeight`/`scrollHeight` no longer reflect the content's natural height.
// Reading those would make the window grow a little on every engine switch.
// Instead we measure the panel's *content* (its `.settings-section` children),
// which is unaffected by how tall the panel is stretched.
function applyPin() {
  const pinned = Boolean(state.settings && state.settings.pinOnTop);
  const btn = document.querySelector("#pin-btn");
  if (btn) {
    btn.classList.toggle("on", pinned);
    btn.setAttribute("aria-pressed", pinned ? "true" : "false");
    btn.title = pinned ? "取消置顶" : "置顶窗口";
    // 按钮里现在只有图标没有文字，辅助技术只能读到 label，所以状态变了要一起改。
    btn.setAttribute("aria-label", pinned ? "取消置顶" : "置顶窗口");
  }
  // 置顶状态是跟着设置存的，所以每次渲染都把它回写到窗口上：换设备/手改过
  // settings.json 之后，按钮和窗口不会各说各话。
  invoke?.("set_pin_on_top", { pinned }).catch(() => {});
}

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
    const text = state.inputText.trim();
    if (text) {
      translateText();
    } else {
      // Input cleared: cancel any in-flight result and reset the output.
      translateSeq++;
      state.textLoading = false;
      state.translatedText = "";
      state.alternatives = [];
      state.detectedLang = "";
      updateOutputArea();
      updateDetectedLang();
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
  inputText: "",
  translatedText: "",
  alternatives: [],
  textLoading: false,
  detectedLang: "",
  ttsPlaying: false,
  hotkeyRecording: false,
  settingsOpen: false
};

function defaultSettings() {
  return {
    fromLang: "auto",
    toLang: "zh-CHS",
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
      autoPrompt: "You are a professional translator. Detect the source language and translate the following text to {to}. Only output the translation, nothing else. Do not add explanations or notes."
    },
    popupShortcut: null,
    proxyMode: "system",
    customProxy: "",
    pinOnTop: false
  };
}

/* ── Helpers ── */

function escapeHtml(v) {
  return String(v).replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;")
    .replaceAll('"',"&quot;").replaceAll("'","&#39;");
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

function languageOptions(current) {
  return LANGUAGES.map(l =>
    `<option value="${l.value}" ${l.value === current ? "selected" : ""}>${l.label}</option>`
  ).join("");
}

function fallbackTargetLang(fromLang) {
  const detected = state.detectedLang;
  if (detected && detected !== "auto" && detected !== fromLang) return detected;
  return fromLang === "zh-CHS" ? "en" : "zh-CHS";
}

function swapLanguages() {
  if (!state.settings) return;
  const from = state.settings.fromLang;
  const to = state.settings.toLang;
  const nextFrom = to === "auto" ? fallbackTargetLang(from) : to;
  const nextTo = from === "auto" ? fallbackTargetLang(nextFrom) : from;
  state.settings.fromLang = nextFrom;
  state.settings.toLang = nextTo;
  const fromEl = document.querySelector("#from-lang");
  const toEl = document.querySelector("#to-lang");
  if (fromEl) fromEl.value = nextFrom;
  if (toEl) toEl.value = nextTo;
  saveSettings().catch(() => {});
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
  const attempts = (window.__TAURI__ || window.__TAURI_INTERNALS__) ? 100 : 5;
  for (let i = 0; i < attempts; i++) {
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
  // Browser preview: keep the UI usable when the Tauri runtime is absent.
  invoke = async (cmd, args) => {
    if (cmd === "load_settings") return defaultSettings();
    if (cmd === "save_settings") return args?.settings || defaultSettings();
    if (cmd === "list_history") return [];
    if (cmd === "resize_main_window" || cmd === "hide_window" || cmd === "close_overlay") return null;
    throw new Error("当前是浏览器预览，截图/翻译需要桌面应用");
  };
  listen = async () => () => {};
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
    updateOutputArea();
  });
  await listen("main:focus-text-input", () => {
    focusTextInputIfAllowed({
      mode,
      hotkeyRecording: state.hotkeyRecording,
      input: document.querySelector("#text-input"),
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
            <select id="from-lang">${languageOptions(state.settings.fromLang)}</select>
            <button type="button" class="lang-swap" id="lang-swap" title="交换源语言和目标语言" aria-label="交换源语言和目标语言">⇄</button>
            <select id="to-lang">${languageOptions(state.settings.toLang)}</select>
          </div>
        </div>
        <div class="header-right">
          <button class="pin-btn${state.settings.pinOnTop ? " on" : ""}" id="pin-btn" title="置顶窗口" aria-label="置顶窗口" aria-pressed="${state.settings.pinOnTop ? "true" : "false"}"><svg class="pin-ico" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false"><rect x="3" y="2.2" width="10" height="4.4" rx="1.8"/><rect x="7" y="6.2" width="2" height="5.4" rx="1"/><path d="M6.3 11.6h3.4L8 14.4z"/></svg></button>
          <button class="capture-btn" id="capture-btn" title="截图翻译">⛶ <span class="btn-label">截图翻译</span></button>
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
            <button class="tts-btn ${state.ttsPlaying ? "speaking" : ""}" id="tts-btn" title="朗读">🔊</button>
          </div>
        </div>
        <div class="text-col output-col">
          <div class="output-wrap" id="output-wrap">
            <div class="output-primary" id="output-primary"></div>
            <div class="output-alternatives" id="output-alternatives"></div>
          </div>
        </div>
      </div>

      <div class="settings-panel" id="settings-panel" style="${state.settingsOpen ? "" : "display:none"}">
        <div class="settings-section">
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
        <div class="settings-section" id="llm-settings" style="${state.settings.textTranslateEngine === "llm" ? "" : "display:none"}">
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
    </div>`;

  // Restore input
  const inp = document.querySelector("#text-input");
  inp.value = state.inputText;

// Events
  // Tracked IME state: `input` events keep firing while the user is still
  // picking candidates, so the events alone are not enough to know when text
  // has actually been committed. See ./ime-guards.mjs.
  let composing = false;
  inp.addEventListener("compositionstart", () => { composing = true; });
  inp.addEventListener("compositionend", () => {
    composing = false;
    state.inputText = inp.value;
    debouncedTranslate();
  });
  inp.addEventListener("input", e => {
    state.inputText = e.target.value;
    // Translate committed text only — never the raw pinyin/kana buffer.
    if (!shouldTranslateOnInput(e, composing)) return;
    debouncedTranslate();
  });
  inp.addEventListener("keydown", e => {
    // While the IME owns the keystroke, Enter belongs to the composition: let it
    // through untouched so the candidate is committed instead of being eaten by
    // preventDefault() (which also translated the unfinished buffer).
    if (!shouldTranslateOnEnter(e, composing)) return;
    e.preventDefault();
    translateText();
  });
  inp.addEventListener("paste", () => {
    setTimeout(() => {
      state.inputText = inp.value;
      translateText();
    }, 0);
  });
  document.querySelector("#from-lang").addEventListener("change", e => { state.settings.fromLang = e.target.value; saveSettings().catch(()=>{}); });
  document.querySelector("#to-lang").addEventListener("change", e => { state.settings.toLang = e.target.value; saveSettings().catch(()=>{}); });
  document.querySelector("#lang-swap").addEventListener("click", e => {
    e.stopPropagation();
    swapLanguages();
  });

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

  // 置顶：状态从 settings 走，按钮高亮和窗口实际 topmost 都由 applyPin 同步。
  document.querySelector("#pin-btn").addEventListener("click", e => {
    e.stopPropagation();
    state.settings.pinOnTop = !state.settings.pinOnTop;
    applyPin();
    saveSettings().catch(() => {});
  });
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
  if (promptInput) promptInput.addEventListener("change", e => { state.settings.llmConfig.prompt = e.target.value; saveSettings().catch(() => {}); });
  if (autoPromptInput) autoPromptInput.addEventListener("change", e => { state.settings.llmConfig.autoPrompt = e.target.value; saveSettings().catch(() => {}); });

  // 启动时把已保存的置顶状态回写到窗口上（Rust 侧 setup 阶段窗口可能还没建好）。
  applyPin();
  inp.focus();
}

/* ── Partial DOM updates ── */

function updateOutputArea() {
  const primaryEl = document.querySelector("#output-primary");
  const altEl = document.querySelector("#output-alternatives");
  if (!primaryEl) return;

  if (state.textLoading) {
    primaryEl.innerHTML = `<span class="dot-loading"><span></span><span></span><span></span></span> 翻译中…`;
    if (altEl) altEl.innerHTML = "";
  } else if (state.translatedText) {
    primaryEl.textContent = state.translatedText;
    if (altEl && state.alternatives.length > 0) {
      altEl.innerHTML = state.alternatives
        .map(a => `<span class="alt-tag">${escapeHtml(a)}</span>`)
        .join("");
    } else if (altEl) {
      altEl.innerHTML = "";
    }
  } else if (state.loading && state.status) {
    primaryEl.textContent = state.status;
    if (altEl) altEl.innerHTML = "";
  } else if (state.status) {
    primaryEl.textContent = state.status;
    if (altEl) altEl.innerHTML = "";
  } else {
    primaryEl.textContent = "";
    if (altEl) altEl.innerHTML = "";
  }

  const btn = document.querySelector("#capture-btn");
  if (btn) btn.disabled = state.loading;
}

function updateDetectedLang() {
  const el = document.querySelector("#detected-lang");
  if (!el) return;
  if (state.detectedLang) {
    const label = LANGUAGES.find(l => l.value === state.detectedLang)?.label || state.detectedLang;
    el.textContent = `检测: ${label}`;
    el.style.display = "";
  } else {
    el.style.display = "none";
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
    updateOutputArea();
  }
}

async function translateText() {
  const text = state.inputText.trim();
  if (!text) return;

  // Claim this as the latest request; older in-flight ones become stale.
  const seq = ++translateSeq;

  state.textLoading = true;
  state.translatedText = "";
  state.alternatives = [];
  state.detectedLang = "";
  updateOutputArea();

  // Validate LLM config
  if (state.settings.textTranslateEngine === "llm" && !state.settings.llmConfig.apiKey) {
    if (seq === translateSeq) {
      state.textLoading = false;
      state.status = "请先在设置中配置 API Key";
      state.statusType = "error";
      updateOutputArea();
    }
    return;
  }

  try {
    const r = await invoke("translate_text", { text, fromLang: state.settings.fromLang, toLang: state.settings.toLang });
    if (seq !== translateSeq) return; // superseded by a newer request
    state.translatedText = r.translatedText;
    state.alternatives = r.alternatives || [];
    state.detectedLang = r.fromLangDetected;
    updateDetectedLang();
  } catch (err) {
    if (seq !== translateSeq) return; // superseded; ignore stale error
    state.status = String(err);
    state.statusType = "error";
  } finally {
    // Only the latest request controls the loading state / final render.
    if (seq === translateSeq) {
      state.textLoading = false;
      updateOutputArea();
    }
  }
}

function speakInput() {
  const text = state.inputText.trim();
  if (!text) return;

  if (window.speechSynthesis.speaking) {
    window.speechSynthesis.cancel();
    state.ttsPlaying = false;
    const btn = document.querySelector("#tts-btn");
    if (btn) btn.classList.remove("speaking");
    return;
  }

  const utt = new SpeechSynthesisUtterance(text);
  const fromLang = state.detectedLang || state.settings.fromLang;
  utt.lang = TTS_LANG_MAP[fromLang] || fromLang;
  utt.onend = () => { state.ttsPlaying = false; const b = document.querySelector("#tts-btn"); if (b) b.classList.remove("speaking"); };
  utt.onerror = utt.onend;

  state.ttsPlaying = true;
  const btn = document.querySelector("#tts-btn");
  if (btn) btn.classList.add("speaking");
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
