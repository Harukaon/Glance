const app = document.querySelector("#app");
document.body.dataset.mode = "capture";

let invoke;

const state = {
  payload: null,
  dragging: false,
  dragStart: null,
  dragCurrent: null,
  activePointerId: null,
  pointerMoved: false,
  selectionCss: null,
  selectionImage: null,
  loading: false,
  copyTextMode: false,
  previewReady: false,
  resultImageBase64: "",
  showResult: true,
  error: ""
};

const HINT_PREPARING = "正在准备截图… · Esc/右键/双指取消";
const HINT_SELECT = "拖拽或滑动选择区域 · Esc/右键/双指取消";
const TOUCH_TAP_SLOP = 10;

const bootStartedAt = performance.now();
let timelineSeq = 0;

function escapeHtml(v) {
  return String(v).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;").replaceAll("'", "&#39;");
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function debugTimeline(message) {
  if (!invoke) return;
  const elapsed = (performance.now() - bootStartedAt).toFixed(1);
  const seq = ++timelineSeq;
  try {
    await invoke("capture_debug_log", {
      message: `#${seq} +${elapsed}ms ${message}`
    });
  } catch (_err) {
  }
}

function debugTimelineSoon(message) {
  void debugTimeline(message);
}

function renderFatal(msg) {
  app.innerHTML = `<div class="capture-error-screen">${escapeHtml(msg)}</div>`;
}

async function ensureTauriApi() {
  if (invoke) return;
  for (let i = 0; i < 100; i++) {
    const t = window.__TAURI__;
    if (t?.core?.invoke) {
      invoke = t.core.invoke;
      return;
    }
    await delay(20);
  }
  throw new Error("Tauri runtime unavailable");
}

function getRootRect() {
  return document.querySelector("#capture-root")?.getBoundingClientRect() || new DOMRect(0, 0, window.innerWidth, window.innerHeight);
}

function clamp(value, min, max) {
  return Math.min(Math.max(value, min), max);
}

function normalizeRect(a, b, bounds) {
  const x1 = clamp(Math.min(a.x, b.x), 0, bounds.width);
  const y1 = clamp(Math.min(a.y, b.y), 0, bounds.height);
  const x2 = clamp(Math.max(a.x, b.x), 0, bounds.width);
  const y2 = clamp(Math.max(a.y, b.y), 0, bounds.height);
  return { x: x1, y: y1, width: x2 - x1, height: y2 - y1 };
}

function cssRectToImageRect(rect) {
  const bounds = getRootRect();
  return {
    x: Math.round(rect.x * state.payload.imageWidth / bounds.width),
    y: Math.round(rect.y * state.payload.imageHeight / bounds.height),
    width: Math.round(rect.width * state.payload.imageWidth / bounds.width),
    height: Math.round(rect.height * state.payload.imageHeight / bounds.height)
  };
}

function imageRectToCssRect(rect) {
  const bounds = getRootRect();
  return {
    x: rect.x * bounds.width / state.payload.imageWidth,
    y: rect.y * bounds.height / state.payload.imageHeight,
    width: rect.width * bounds.width / state.payload.imageWidth,
    height: rect.height * bounds.height / state.payload.imageHeight
  };
}

function setError(message) {
  state.error = message ? String(message) : "";
  const errorEl = document.querySelector("#capture-error");
  if (!errorEl) return;
  errorEl.hidden = !state.error;
  errorEl.textContent = state.error;
}

function updatePreviewImages() {
  const screenEl = document.querySelector("#capture-screen");
  const previewEl = document.querySelector("#capture-selection-preview");
  const hintEl = document.querySelector("#capture-hint");
  if (!screenEl || !previewEl || !hintEl) return;

  if (!state.payload) {
    screenEl.removeAttribute("src");
    previewEl.removeAttribute("src");
    hintEl.textContent = HINT_PREPARING;
    return;
  }

  const src = `data:${state.payload.imageMime};base64,${state.payload.imageBase64}`;
  if (screenEl.dataset.debugSrc !== src) {
    screenEl.dataset.debugSrc = src;
    debugTimelineSoon(`screen image src assigned mime=${state.payload.imageMime} size=${state.payload.imageWidth}x${state.payload.imageHeight}`);
  }
  screenEl.src = src;
  previewEl.src = src;
  state.previewReady = true;
  if (!state.selectionCss && !state.loading) {
    hintEl.textContent = HINT_SELECT;
  }
}

function loadingHintText() {
  return state.copyTextMode ? "正在识别文字…" : "正在翻译…";
}

function updateSelectionLayer() {
  const selectionEl = document.querySelector("#capture-selection");
  const previewEl = document.querySelector("#capture-selection-preview");
  const resultEl = document.querySelector("#capture-selection-result");
  const spinnerEl = document.querySelector("#capture-spinner");
  const hintEl = document.querySelector("#capture-hint");

  if (!selectionEl || !previewEl || !resultEl || !spinnerEl || !hintEl) return;

  if (!state.selectionCss) {
    selectionEl.hidden = true;
    hintEl.textContent = state.previewReady ? HINT_SELECT : HINT_PREPARING;
    return;
  }

  selectionEl.hidden = false;
  selectionEl.style.left = `${state.selectionCss.x}px`;
  selectionEl.style.top = `${state.selectionCss.y}px`;
  selectionEl.style.width = `${state.selectionCss.width}px`;
  selectionEl.style.height = `${state.selectionCss.height}px`;

  previewEl.style.width = `${window.innerWidth}px`;
  previewEl.style.height = `${window.innerHeight}px`;
  previewEl.style.transform = `translate(${-state.selectionCss.x}px, ${-state.selectionCss.y}px)`;

  spinnerEl.hidden = !state.loading;
  if (state.resultImageBase64) {
    if (state.showResult) {
      resultEl.src = `data:image/jpeg;base64,${state.resultImageBase64}`;
      resultEl.hidden = false;
      previewEl.hidden = true;
    } else {
      resultEl.hidden = true;
      previewEl.hidden = false;
    }
    hintEl.textContent = "区域内右键切换原文/译文 · Esc或区域外右键关闭";
  } else {
    resultEl.hidden = true;
    previewEl.hidden = false;
    hintEl.textContent = state.loading ? loadingHintText() : HINT_SELECT;
  }
}

async function cancelCapture() {
  try {
    await invoke("cancel_capture");
  } catch (err) {
    renderFatal(err);
  }
}

function pointFromEvent(event) {
  const bounds = getRootRect();
  return {
    x: event.clientX - bounds.left,
    y: event.clientY - bounds.top
  };
}

function pointerWithinSelection(event) {
  if (!state.selectionCss) return false;
  const p = pointFromEvent(event);
  const r = state.selectionCss;
  return p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height;
}

async function submitSelection(rectCss) {
  const rect = cssRectToImageRect(rectCss);
  if (rect.width <= 4 || rect.height <= 4) {
    state.selectionCss = null;
    state.selectionImage = null;
    updateSelectionLayer();
    return;
  }

  state.selectionCss = rectCss;
  state.selectionImage = rect;
  state.loading = true;
  state.resultImageBase64 = "";
  setError("");
  updateSelectionLayer();

  try {
    const result = await invoke("submit_capture_selection", { selection: rect });
    state.selectionImage = result.selection;
    state.selectionCss = imageRectToCssRect(result.selection);
    state.resultImageBase64 = result.imageBase64;
    state.showResult = true;
  } catch (err) {
    setError(err);
  } finally {
    state.loading = false;
    updateSelectionLayer();
  }
}

function applySecondaryAction(event) {
  if (state.resultImageBase64 && pointerWithinSelection(event)) {
    state.showResult = !state.showResult;
    updateSelectionLayer();
    return;
  }
  cancelCapture();
}

function beginSelectionDrag(event) {
  state.dragging = true;
  state.activePointerId = event.pointerId;
  state.pointerMoved = false;
  state.dragStart = pointFromEvent(event);
  state.dragCurrent = state.dragStart;
  if (!state.resultImageBase64) {
    state.selectionCss = null;
    state.selectionImage = null;
  }
  setError("");
  updateSelectionLayer();
}

function bindCaptureEvents() {
  const root = document.querySelector("#capture-root");
  if (!root) return;

  root.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    // When a translation result is shown and the right-click lands inside the
    // selected region, toggle between translation and original text instead of
    // cancelling. Right-click outside the region (or with no result) cancels.
    applySecondaryAction(event);
  });

  root.addEventListener("pointerdown", (event) => {
    if (state.loading || !state.previewReady) return;
    if (state.activePointerId != null && event.pointerId !== state.activePointerId) {
      event.preventDefault();
      applySecondaryAction(event);
      return;
    }
    if (event.button !== 0) return;
    event.preventDefault();
    try { root.setPointerCapture(event.pointerId); } catch (_err) {}
    beginSelectionDrag(event);
  });

  root.addEventListener("pointermove", (event) => {
    if (!state.dragging || event.pointerId !== state.activePointerId) return;
    event.preventDefault();
    const next = pointFromEvent(event);
    if (!state.pointerMoved && state.dragStart) {
      if (Math.abs(next.x - state.dragStart.x) > TOUCH_TAP_SLOP
          || Math.abs(next.y - state.dragStart.y) > TOUCH_TAP_SLOP) {
        state.pointerMoved = true;
        if (state.resultImageBase64) {
          state.resultImageBase64 = "";
          state.selectionCss = null;
          state.selectionImage = null;
        }
      }
    }
    state.dragCurrent = next;
    if (state.pointerMoved || !state.resultImageBase64) {
      state.selectionCss = normalizeRect(state.dragStart, state.dragCurrent, getRootRect());
    }
    updateSelectionLayer();
  });

  root.addEventListener("pointerup", async (event) => {
    if (event.pointerId !== state.activePointerId) return;
    if (event.button === 2) {
      state.dragging = false;
      state.activePointerId = null;
      return;
    }
    if (event.button !== 0 || !state.dragging) return;
    event.preventDefault();
    state.dragging = false;
    state.activePointerId = null;
    state.dragCurrent = pointFromEvent(event);
    if (!state.pointerMoved && state.resultImageBase64) {
      applySecondaryAction(event);
      return;
    }
    const rectCss = normalizeRect(state.dragStart, state.dragCurrent, getRootRect());
    await submitSelection(rectCss);
  });

  root.addEventListener("pointercancel", (event) => {
    if (event.pointerId !== state.activePointerId) return;
    state.dragging = false;
    state.activePointerId = null;
    state.pointerMoved = false;
    if (!state.resultImageBase64) {
      state.selectionCss = null;
      state.selectionImage = null;
    }
    updateSelectionLayer();
  });

  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      cancelCapture();
    }
  });

  window.addEventListener("resize", () => {
    if (state.selectionImage) {
      state.selectionCss = imageRectToCssRect(state.selectionImage);
      updateSelectionLayer();
    }
  });
}

function renderCapture() {
  app.innerHTML = `
    <div class="capture-root" id="capture-root">
      <img class="capture-screen capture-screen-dim" id="capture-screen" alt="" />
      <div class="capture-selection" id="capture-selection" hidden>
        <img class="capture-selection-preview" id="capture-selection-preview" alt="" />
        <img class="capture-selection-result" id="capture-selection-result" alt="" hidden />
        <div class="capture-spinner" id="capture-spinner" hidden></div>
      </div>
      <div class="capture-hint" id="capture-hint">正在准备截图… · Esc/右键/双指取消</div>
      <div class="capture-error" id="capture-error" hidden></div>
    </div>`;

  const screenEl = document.querySelector("#capture-screen");
  if (screenEl) {
    screenEl.addEventListener("load", () => {
      debugTimelineSoon(`screen image load natural=${screenEl.naturalWidth}x${screenEl.naturalHeight}`);
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          debugTimelineSoon("screen image painted");
        });
      });
    });
  }

  bindCaptureEvents();
  updatePreviewImages();
  updateSelectionLayer();
}

async function boot() {
  await ensureTauriApi();
  await debugTimeline("tauri api ready");
  renderCapture();
  debugTimelineSoon("capture DOM rendered");
  const payloadStartedAt = performance.now();
  for (let i = 0; i < 200; i++) {
    try {
      state.payload = await invoke("load_capture_payload");
      state.copyTextMode = Boolean(state.payload.copyTextMode);
      debugTimelineSoon(`load_capture_payload success attempts=${i + 1} wait=${(performance.now() - payloadStartedAt).toFixed(1)}ms`);
      updatePreviewImages();
      break;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (!message.includes("capture preview not ready") && !message.includes("capture payload missing")) {
        throw err;
      }
      await delay(25);
    }
  }
  if (!state.payload) {
    throw new Error("capture preview timed out");
  }
  debugTimelineSoon("capture payload applied");
}

window.addEventListener("error", (event) => {
  debugTimelineSoon(`window error ${event.error?.message || event.message || "unknown"}`);
  renderFatal(event.error?.message || event.message || "unknown");
});
window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason instanceof Error ? event.reason.message : String(event.reason);
  debugTimelineSoon(`unhandled rejection ${reason}`);
  renderFatal(reason);
});
boot().catch((err) => renderFatal(err instanceof Error ? err.message : String(err)));
