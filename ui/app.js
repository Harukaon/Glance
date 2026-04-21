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
  { value: "llm", label: "AI 大模型" }
];

let debounceTimer = null;

function debouncedTranslate() {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    const text = state.inputText.trim();
    if (text && !state.textLoading) translateText();
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
    textTranslateEngine: "bing",
    llmConfig: {
      baseUrl: "https://api.openai.com",
      apiKey: "",
      model: "gpt-4o-mini"
    },
    popupShortcut: null
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
    <div class="bento-app">
      <div class="block header-block" data-tauri-drag-region>
        <div class="header-left">
          <div class="app-title">Glance</div>
          <div class="lang-pill">
            <select id="from-lang">${languageOptions(state.settings.fromLang)}</select>
            <span class="lang-icon">➔</span>
            <select id="to-lang">${languageOptions(state.settings.toLang)}</select>
          </div>
        </div>
        <div class="header-right">
          <button class="capture-btn" id="capture-btn" title="截图翻译">⛶ 截图翻译</button>
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
            <span class="settings-label">弹出窗口</span>
            <div class="shortcut-row settings-shortcut" id="popup-shortcut-row">
              ${state.settings.popupShortcut ? shortcutKeysHtml(state.settings.popupShortcut) : "未设置"} <span class="shortcut-hint">点击可设置</span>
            </div>
          </div>
        </div>
        <div class="settings-section" id="llm-settings" style="${state.settings.textTranslateEngine === "llm" ? "" : "display:none"}">
          <div class="settings-row">
            <span class="settings-label">API 地址</span>
            <input class="settings-input" id="llm-base-url" type="text"
                    value="${escapeHtml(state.settings.llmConfig.baseUrl)}"
                    placeholder="https://api.openai.com" />
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
        </div>
      </div>
    </div>`;

  // Restore input
  const inp = document.querySelector("#text-input");
  inp.value = state.inputText;

// Events
  inp.addEventListener("input", e => { state.inputText = e.target.value; debouncedTranslate(); });
  inp.addEventListener("keydown", e => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); translateText(); }
  });
  inp.addEventListener("paste", () => {
    setTimeout(() => {
      state.inputText = inp.value;
      translateText();
    }, 0);
  });
  document.querySelector("#from-lang").addEventListener("change", e => { state.settings.fromLang = e.target.value; saveSettings().catch(()=>{}); });
  document.querySelector("#to-lang").addEventListener("change", e => { state.settings.toLang = e.target.value; saveSettings().catch(()=>{}); });

  document.querySelector("#settings-btn").addEventListener("click", e => {
    e.stopPropagation();
    state.settingsOpen = !state.settingsOpen;
    const panel = document.querySelector("#settings-panel");
    const btn = e.currentTarget;
    panel.style.display = state.settingsOpen ? "" : "none";
    btn.classList.toggle("open", state.settingsOpen);
    if (state.settingsOpen) {
      const h = state.settings.textTranslateEngine === "llm" ? 620 : 520;
      invoke?.("resize_main_window", { height: h }).catch(() => {});
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

  document.querySelector("#capture-btn").addEventListener("click", e => { e.stopPropagation(); startCapture(); });
  document.querySelector("#shortcut-row").addEventListener("click", e => { e.stopPropagation(); startHotkeyRecording(); });
  document.querySelector("#popup-shortcut-row").addEventListener("click", e => { e.stopPropagation(); startPopupShortcutRecording(); });

  // Engine switcher
  document.querySelectorAll(".engine-btn").forEach(btn => {
    btn.addEventListener("click", e => {
      e.stopPropagation();
      const newEngine = e.currentTarget.dataset.engine;
      state.settings.textTranslateEngine = newEngine;
      saveSettings().catch(() => {});
      // Update active state
      document.querySelectorAll(".engine-btn").forEach(b => b.classList.toggle("active", b.dataset.engine === newEngine));
      // Toggle LLM settings visibility
      const llmSettings = document.querySelector("#llm-settings");
      if (llmSettings) llmSettings.style.display = newEngine === "llm" ? "" : "none";
      // Resize window
      const h = newEngine === "llm" ? 620 : 520;
      invoke?.("resize_main_window", { height: h }).catch(() => {});
    });
  });

  // LLM config inputs
  const baseUrlInput = document.querySelector("#llm-base-url");
  const apiKeyInput = document.querySelector("#llm-api-key");
  const modelInput = document.querySelector("#llm-model");
  if (baseUrlInput) baseUrlInput.addEventListener("change", e => { state.settings.llmConfig.baseUrl = e.target.value.trim(); saveSettings().catch(() => {}); });
  if (apiKeyInput) apiKeyInput.addEventListener("change", e => { state.settings.llmConfig.apiKey = e.target.value.trim(); saveSettings().catch(() => {}); });
  if (modelInput) modelInput.addEventListener("change", e => { state.settings.llmConfig.model = e.target.value.trim(); saveSettings().catch(() => {}); });

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
  if (!text || state.textLoading) return;

  state.textLoading = true;
  state.translatedText = "";
  state.alternatives = [];
  state.detectedLang = "";
  updateOutputArea();

  // Validate LLM config
  if (state.settings.textTranslateEngine === "llm" && !state.settings.llmConfig.apiKey) {
    state.textLoading = false;
    state.status = "请先在设置中配置 API Key";
    state.statusType = "error";
    updateOutputArea();
    return;
  }

  try {
    const r = await invoke("translate_text", { text, fromLang: state.settings.fromLang, toLang: state.settings.toLang });
    state.translatedText = r.translatedText;
    state.alternatives = r.alternatives || [];
    state.detectedLang = r.fromLangDetected;
    updateDetectedLang();
  } catch (err) {
    state.status = String(err);
    state.statusType = "error";
  } finally {
    state.textLoading = false;
    updateOutputArea();
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
    if (e.ctrlKey) mods.push("Ctrl");
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
    if (e.ctrlKey) mods.push("Ctrl");
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
