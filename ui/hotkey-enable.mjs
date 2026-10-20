// Enable switch per shortcut row (#23): switching off parks the binding — hotkey
// unregistered, combo string kept, so it can be switched back on without re-recording.

export function rowState({ combo, enabled }) {
  if (!combo) return "unset";   // nothing bound wins over the switch
  return enabled ? "armed" : "disabled";
}

export function rowClasses({ combo, enabled }) {
  return {
    toggle: enabled ? "on" : "",
    binding: rowState({ combo, enabled }) === "disabled" ? "shortcut-off" : "",
  };
}

export function shouldEnterRecording({ target }) {
  return !target?.closest?.("[data-hotkey-enable]");
}

export function recordedBinding(combo) {
  return { combo, enabled: true };
}
