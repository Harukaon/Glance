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
  textLoading: false,
  detectedLang: "",
  ttsPlaying: false,
  hotkeyRecording: false
};

/* ── Helpers ── */

function escapeHtml(v) {
  return String(v).replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;")
    .replaceAll('"',"&quot;").replaceAll("'","&#39;");
}

function delay(ms) { return new Promise(r => setTimeout(r, ms)); }

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
    if (t?.core?.invoke && t?.event?.listen) { invoke = t.core.invoke; listen = t.event.listen; return; }
    await delay(20);
  }
  throw new Error("Tauri runtime unavailable");
}

async function loadSettings() { state.settings = await invoke("load_settings"); }
async function saveSettings() { state.settings = await invoke("save_settings", { settings: state.settings }); }

async function bindMainListeners() {
  if (state.listenersBound || mode !== "main") return;
  await listen("workflow:state", (event) => {
    const p = event.payload || {};
    state.loading = Boolean(p.busy);
    if (typeof p.message === "string") { state.status = p.message; state.statusType = p.type || ""; }
    updateOutputArea();
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
          <span class="autostart-label">开机自启</span>
          <button class="toggle ${state.settings.autostart ? "on" : ""}" id="autostart" title="开机自启" aria-pressed="${state.settings.autostart}"></button>
        </div>
      </div>

      <div class="block text-block">
        <div class="input-wrap">
          <textarea class="input-area" id="text-input" placeholder="输入要翻译的文本..." rows="3"></textarea>
          <button class="tts-btn ${state.ttsPlaying ? "speaking" : ""}" id="tts-btn" title="朗读">🔊</button>
        </div>
        <div class="divider"></div>
        <div class="meta-info">
          <span class="detect-tag" id="detected-lang" style="display:none"></span>
        </div>
        <textarea class="output-area" id="output-area" readonly></textarea>
      </div>

      <div class="block action-block" id="action-block">
        <div class="action-info">
          <div class="action-title">截图翻译</div>
          <div class="shortcut-row" id="shortcut-row">
            快捷键: ${shortcutKeysHtml(state.settings.hotkey)} <span class="shortcut-hint">点击可设置快捷键</span>
          </div>
        </div>
        <button class="action-icon" id="capture-btn" title="截图翻译">⛶</button>
      </div>
    </div>`;

  // Restore input
  const inp = document.querySelector("#text-input");
  inp.value = state.inputText;

  // Update output & detected lang
  updateOutputArea();
  updateDetectedLang();

  // Events
  inp.addEventListener("input", e => { state.inputText = e.target.value; });
  inp.addEventListener("keydown", e => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); translateText(); }
  });
  document.querySelector("#from-lang").addEventListener("change", e => { state.settings.fromLang = e.target.value; saveSettings().catch(()=>{}); });
  document.querySelector("#to-lang").addEventListener("change", e => { state.settings.toLang = e.target.value; saveSettings().catch(()=>{}); });
  document.querySelector("#autostart").addEventListener("click", e => {
    state.settings.autostart = !state.settings.autostart;
    e.currentTarget.classList.toggle("on", state.settings.autostart);
    e.currentTarget.setAttribute("aria-pressed", state.settings.autostart);
    saveSettings().catch(() => {});
  });
  document.querySelector("#tts-btn").addEventListener("click", speakInput);
  document.querySelector("#capture-btn").addEventListener("click", e => { e.stopPropagation(); startCapture(); });
  document.querySelector("#action-block").addEventListener("click", () => { startCapture(); });
  document.querySelector("#shortcut-row").addEventListener("click", e => { e.stopPropagation(); startHotkeyRecording(); });

  inp.focus();
}

/* ── Partial DOM updates ── */

function updateOutputArea() {
  const el = document.querySelector("#output-area");
  if (!el) return;
  const dots = `<span class="dot-loading"><span></span><span></span><span></span></span>`;

  if (state.textLoading) {
    el.value = "";
    el.parentElement.querySelector(".dot-loading")?.remove();
    const loader = document.createElement("span");
    loader.className = "dot-loading";
    loader.innerHTML = "<span></span><span></span><span></span>";
    el.before(loader);
  } else {
    el.parentElement.querySelector(".dot-loading")?.remove();
    if (state.translatedText) {
      el.value = state.translatedText;
    } else if (state.loading && state.status) {
      el.value = state.status;
    } else if (state.status) {
      el.value = state.status;
    } else {
      el.value = "";
    }
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
  try {
    state.loading = true;
    state.status = "正在截图…";
    state.statusType = "";
    updateOutputArea();
    await saveSettings();
    await invoke("begin_capture", { options: { fromLang: state.settings.fromLang, toLang: state.settings.toLang } });
  } catch (err) {
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
  state.detectedLang = "";
  updateOutputArea();

  try {
    const r = await invoke("translate_text", { text, fromLang: state.settings.fromLang, toLang: state.settings.toLang });
    state.translatedText = r.translatedText;
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
    let key = KEY_MAP[e.key] || (e.key.length === 1 ? e.key.toUpperCase() : null);
    if (!key || mods.length === 0) {
      if (e.key === "Escape") { finishRecording(null); }
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
    document.querySelector("#overlay-stage").innerHTML = `
      <img class="overlay-image" src="${src}" alt="translated"
           style="left:${s.x}px;top:${s.y}px;width:${s.width}px;height:${s.height}px;opacity:${state.overlay.overlayOpacity};" />`;
  } catch (err) { renderFatal(`覆盖层初始化失败: ${err}`); }
}

/* ── Boot ── */

document.addEventListener("keydown", e => {
  if (e.key === "Escape" && mode === "main" && !state.hotkeyRecording) {
    invoke?.("hide_window");
  }
});

async function boot() {
  await ensureTauriApi();
  await bindMainListeners();
  if (mode.startsWith("overlay")) { await renderOverlay(); return; }
  await loadSettings();
  renderMain();
}

window.addEventListener("error", e => renderFatal(e.error?.message || e.message || "unknown"));
window.addEventListener("unhandledrejection", e => renderFatal(e.reason instanceof Error ? e.reason.message : String(e.reason)));
boot().catch(e => renderFatal(e instanceof Error ? e.message : String(e)));
