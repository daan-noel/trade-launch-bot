// A dynamic metric group's **window span** — the frontend mirror of
// `hunter_engine::metrics::WindowSpec`.
//
// A window is three things, not one: how wide it is, how far back it ENDS, and the
// unit both are counted in. Time is continuous and slots are discrete, so the two
// units are not interchangeable — at ~400 ms a one-second window straddles two or
// three slots and merges bursts that landed separately. Every surface that labels,
// keys, or validates a window reads it through this module, so a slot window can
// never render as a second window or collapse onto one.

/** What a span counts in (wire `WindowUnit`). */
export type WindowUnit = 'sec' | 'slot';

/** One window axis, as the backend serializes it. */
export interface WindowSpec {
  size: number;
  /** Units back from *now* the window ENDS. `0` = it ends at now. */
  lag: number;
  unit: WindowUnit;
}

// ── Param names (SSOT: `hunter_engine::metrics` / `metrics::flow_burst`) ──────

/** Size of a wall-clock window. */
export const WINDOW_SEC_PARAM = 'window_size_sec';
/** Size of a slot window. Mutually exclusive with {@link WINDOW_SEC_PARAM}. */
export const WINDOW_SLOT_PARAM = 'window_size_slots';
/** Units back from now the window ends. Shared by both axes of a two-window group. */
export const WINDOW_LAG_PARAM = 'window_lag';
/** `m_flow_burst`'s second axis, in seconds. */
export const BURST_PARAM = 'burst_size_sec';
/** `m_flow_burst`'s second axis, in slots. Mutually exclusive with {@link BURST_PARAM}. */
export const BURST_SLOT_PARAM = 'burst_size_slots';

/** Every window param — the keys a strict bag owns that are NOT a metric and NOT
 *  something the editor carries opaquely. */
export const WINDOW_PARAMS = [
  WINDOW_SEC_PARAM,
  WINDOW_SLOT_PARAM,
  WINDOW_LAG_PARAM,
  BURST_PARAM,
  BURST_SLOT_PARAM,
] as const;

/** The size param a unit spells itself with. */
export function sizeParam(unit: WindowUnit): string {
  return unit === 'slot' ? WINDOW_SLOT_PARAM : WINDOW_SEC_PARAM;
}

/** The burst-axis size param for a unit. */
export function burstSizeParam(unit: WindowUnit): string {
  return unit === 'slot' ? BURST_SLOT_PARAM : BURST_PARAM;
}

/** Short unit suffix — `s` / `sl`. Must stay identical to the Rust
 *  `event::format_metric_exit_name`, because a persisted exit reason and a live chip
 *  naming the same req have to read the same. */
export function unitSuffix(unit: WindowUnit): string {
  return unit === 'slot' ? 'sl' : 's';
}

/** Long unit name for prose and control labels. */
export function unitLabel(unit: WindowUnit): string {
  return unit === 'slot' ? 'slots' : 'seconds';
}

/**
 * Read one window axis out of a group instance's strict bag.
 *
 * Mirrors `GroupConditions::window_spec`: a slot size wins over a seconds size when
 * both are somehow present (the backend rejects that combination at save), and the
 * lag is the group's — a two-window group is two spans on ONE clock.
 */
export function windowSpecFromStrict(
  strict: Record<string, number> | undefined,
  secParam: string = WINDOW_SEC_PARAM,
  slotParam: string = WINDOW_SLOT_PARAM,
): WindowSpec | null {
  if (!strict) return null;
  const lag = typeof strict[WINDOW_LAG_PARAM] === 'number' ? strict[WINDOW_LAG_PARAM] : 0;
  const slots = strict[slotParam];
  if (typeof slots === 'number' && Number.isFinite(slots) && slots > 0)
    return { size: slots, lag, unit: 'slot' };
  const secs = strict[secParam];
  if (typeof secs === 'number' && Number.isFinite(secs) && secs > 0)
    return { size: secs, lag, unit: 'sec' };
  return null;
}

/** The burst axis of a group instance, if it authors one. */
export function burstSpecFromStrict(
  strict: Record<string, number> | undefined,
): WindowSpec | null {
  return windowSpecFromStrict(strict, BURST_PARAM, BURST_SLOT_PARAM);
}

/**
 * `30s`, `30sl`, `30sl@1` — the span, named. The `@lag` half appears only when
 * there IS a lag, because a lagged window reads a DIFFERENT span from an unlagged
 * one of the same size and the two must never print identically.
 *
 * Same vocabulary as the Rust `format_metric_exit_name`, so a chip, a chart legend
 * and a stored exit reason agree on which req is meant.
 */
export function formatWindowSpec(spec: WindowSpec | null | undefined): string {
  if (!spec || !Number.isFinite(spec.size) || spec.size <= 0) return '';
  const lag = spec.lag > 0 ? `@${trim(spec.lag)}` : '';
  return `${trim(spec.size)}${unitSuffix(spec.unit)}${lag}`;
}

/** Trailing-zero-free number, matching the Rust `format_metric_threshold`. */
function trim(n: number): string {
  return String(Number(n.toFixed(4)));
}

/**
 * Dedup identity of a span — two windows are the same buffer only if size, lag and
 * unit all agree. This is the frontend half of `WindowSpec::key`; using the bare
 * size instead is how two slot windows of one metric silently merge into one group
 * instance and one of the two conditions disappears on save.
 */
export function windowSpecKey(spec: WindowSpec | null | undefined): string {
  if (!spec) return '∅';
  // Millisecond-resolution integers, like the Rust `quantize`, so 0.1 + 0.2 and 0.3
  // are one key rather than two.
  return `${spec.unit}:${Math.round(spec.size * 1000)}:${Math.round(spec.lag * 1000)}`;
}

/** True when two spans are the same window. */
export function sameWindowSpec(
  a: WindowSpec | null | undefined,
  b: WindowSpec | null | undefined,
): boolean {
  return windowSpecKey(a) === windowSpecKey(b);
}

/**
 * Back-compat read of a readout/arm payload that carries BOTH the legacy
 * `window_size_sec` scalar and the full `window` object.
 *
 * The scalar is `null` on a slot window (it has no seconds to report), so a reader
 * that only knows the scalar drops the window entirely rather than mislabelling it
 * — but every reader here should prefer the object when it is present.
 */
export function readWindow(read: {
  window?: WindowSpec | null;
  window_size_sec?: number | null;
}): WindowSpec | null {
  if (read.window && Number.isFinite(read.window.size)) return read.window;
  const w = read.window_size_sec;
  return typeof w === 'number' && Number.isFinite(w) && w > 0
    ? { size: w, lag: 0, unit: 'sec' }
    : null;
}
