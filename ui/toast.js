function delay(ms) { return new Promise(r => setTimeout(r, ms)); }

async function ensureTauriApi() {
  for (let i = 0; i < 100; i++) {
    const t = window.__TAURI__;
    const ti = window.__TAURI_INTERNALS__;
    const nextInvoke = t?.core?.invoke || ti?.invoke;
    if (nextInvoke) { return nextInvoke; }
    await delay(20);
  }
  return null;
}

const invoke = await ensureTauriApi();
if (invoke) {
  setTimeout(() => {
    invoke("close_toast").catch(() => {});
  }, 2000);
}
