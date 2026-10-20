// Settings-panel height policy (#27, #23): grow only, never size from the current height.
export function requiredHeight({ headerH, textBlockMinH, sectionsH, gap, padding } = {}) {
  const n = v => (Number.isFinite(v) ? v : 0);
  const total =
    n(headerH) + n(textBlockMinH) + n(sectionsH) + n(gap) * 2 + n(padding);
  return total > 0 ? Math.ceil(total) : 0;
}

export function nextHeight({ currentH, requiredH, panelOpen, rememberedH, grew } = {}) {
  const n = v => (Number.isFinite(v) ? v : 0);
  const current = n(currentH);
  const required = n(requiredH);
  if (panelOpen) {
    return required > 0 && required > current ? required : null;
  }
  const padded = n(grew);
  if (padded <= 0) return null;
  const target = Math.max(current - padded, n(rememberedH));
  if (target <= 0) return null;
  // innerHeight is ceil(physical / scale): within 2px is rounding noise, not growth.
  const CLOSE_SNAP_PX = 2;
  if (Math.abs(target - current) <= CLOSE_SNAP_PX) return null;
  if (n(rememberedH) > 0 && Math.abs(current - padded - n(rememberedH)) <= CLOSE_SNAP_PX) {
    return n(rememberedH);
  }
  return target;
}

// Monotonic debt: every grow we issued, in the units of the moment it happened.
export function grewAfterResize({ currentH, targetH, previousGrew } = {}) {
  const n = v => (Number.isFinite(v) ? v : 0);
  if (!Number.isFinite(currentH) || !Number.isFinite(targetH)) return n(previousGrew);
  return n(previousGrew) + Math.max(0, targetH - currentH);
}
