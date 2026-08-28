// A dynamic metric group's **window span** — the frontend mirror of
// `hunter_engine::metrics::WindowSpec`.
//
// A window is three things, not one: how wide it is, how far back it ENDS, and the
// unit both are counted in. Time is continuous while slots and prints are discrete,
// so the units are not interchangeable — at ~400 ms a one-second window straddles two
// or three slots and merges bursts that landed separately, and no span on either
// clock can tell ten one-SOL prints from one ten-SOL print. Every surface that
// labels, keys, or validates a window reads it through this module, so a slot or
// print window can never render as a second window or collapse onto one.

/** What a span counts in (wire `WindowUnit`). */
export type WindowUnit = 'sec' | 'slot' | 'print';

/** Every unit, in resolution order — mirrors the Rust `WindowUnit::ALL`. The one
 *  place a basis is enumerated: the unit picker, the param lists and
 *  {@link windowSpecFromStrict} all derive from it. */
export const WINDOW_UNITS: readonly WindowUnit[] = ['sec', 'slot', 'print'] as const;

/** One window axis, as the backend serializes it. */
export interface WindowSpec {
  size: number;
  /** Units back from *now* the window ENDS. `0` = it ends at now. */
  lag: number;
  unit: WindowUnit;
}

// ── Param names (SSOT: `hunter_engine::metrics` / `metrics::flow_slice`) ──────

/** Size of a wall-clock window. */
export const WINDOW_SEC_PARAM = 'window_size_sec';
/** Size of a slot window. Mutually exclusive with {@link WINDOW_SEC_PARAM}. */
export const WINDOW_SLOT_PARAM = 'window_size_slots';
/** Size of a print window — `size` prints of the token's own tape. Mutually
 *  exclusive with the other two. */
export const WINDOW_PRINT_PARAM = 'window_size_prints';
/** Units back from now the window ends. Shared by both axes of a two-window group. */
export const WINDOW_LAG_PARAM = 'window_lag';
/** `m_flow_window`'s second axis, in seconds. */
export const SLICE_PARAM = 'slice_size_sec';
/** `m_flow_window`'s second axis, in slots. Mutually exclusive with {@link SLICE_PARAM}. */
export const SLICE_SLOT_PARAM = 'slice_size_slots';
/** `m_flow_window`'s second axis, in prints. Mutually exclusive with the other two. */
export const SLICE_PRINT_PARAM = 'slice_size_prints';

/** The reference axis's size params — one per unit, in {@link WINDOW_UNITS} order.
 *  Exactly one of these is set on a dynamic group instance. */
export const WINDOW_SIZE_PARAMS = [
  WINDOW_SEC_PARAM,
  WINDOW_SLOT_PARAM,
  WINDOW_PRINT_PARAM,
] as const;

/** `m_flow_window`'s second-axis size params — one per unit, same order and same
 *  "exactly one" rule, and it must resolve to the SAME unit as the reference. */
export const SLICE_SIZE_PARAMS = [SLICE_PARAM, SLICE_SLOT_PARAM, SLICE_PRINT_PARAM] as const;

/** Every window param — the keys a strict bag owns that are NOT a metric and NOT
 *  something the editor carries opaquely. */
export const WINDOW_PARAMS = [
  ...WINDOW_SIZE_PARAMS,
  WINDOW_LAG_PARAM,
  ...SLICE_SIZE_PARAMS,
] as const;

/** The size param a unit spells itself on the reference axis. */
export function sizeParam(unit: WindowUnit): string {
  return unit === 'slot'
    ? WINDOW_SLOT_PARAM
    : unit === 'print'
      ? WINDOW_PRINT_PARAM
      : WINDOW_SEC_PARAM;
}

/** The slice-axis size param for a unit. */
export function sliceSizeParam(unit: WindowUnit): string {
  return unit === 'slot' ? SLICE_SLOT_PARAM : unit === 'print' ? SLICE_PRINT_PARAM : SLICE_PARAM;
}

/** Short unit suffix — `s` / `sl` / `p`. Must stay identical to the Rust
 *  `WindowUnit::suffix`, because a persisted exit reason and a live chip naming the
 *  same req have to read the same. */
export function unitSuffix(unit: WindowUnit): string {
  return unit === 'slot' ? 'sl' : unit === 'print' ? 'p' : 's';
}

/** Long unit name for prose and control labels. */
export function unitLabel(unit: WindowUnit): string {
  return unit === 'slot' ? 'slots' : unit === 'print' ? 'prints' : 'seconds';
}

/** True when a unit's cursor is a discrete counter rather than a clock — so its
 *  sizes are whole buckets and its inputs step by 1, not by half a second. */
export function isDiscreteUnit(unit: WindowUnit): boolean {
  return unit !== 'sec';
}

/** A strict bag with every size param of one axis removed.
 *
 *  Exactly ONE may survive a write — the row's unit picks it — so a caller that
 *  re-spells an axis must clear the whole axis first: a sibling left behind is the
 *  "two spans claiming one axis" the backend rejects at save. Written against
 *  {@link WINDOW_SIZE_PARAMS} / {@link SLICE_SIZE_PARAMS} rather than a destructure
 *  per unit, so a new basis cannot be forgotten at one of these sites. */
export function withoutAxis(
  strict: Record<string, number> | undefined,
  axis: readonly string[],
): Record<string, number> {
  const out = { ...(strict ?? {}) };
  for (const name of axis) delete out[name];
  return out;
}

/** A strict bag with `m_flow_window`'s whole second axis removed. */
export function withoutSliceAxis(
  strict: Record<string, number> | undefined,
): Record<string, number> {
  return withoutAxis(strict, SLICE_SIZE_PARAMS);
}

/**
 * Read one window axis out of a group instance's strict bag.
 *
 * Mirrors `GroupConditions::window_spec`: the axis carries one size param per unit
 * and exactly one of them is set (the backend rejects any other count at save AND at
 * load), so the scan order only ever decides between params that cannot coexist. The
 * lag is the group's — a two-window group is two spans on ONE clock.
 *
 * `axis` maps a unit to that axis's size param: {@link sizeParam} for the reference
 * span, {@link sliceSizeParam} for `m_flow_window`'s second one.
 */
export function windowSpecFromStrict(
  strict: Record<string, number> | undefined,
  axis: (unit: WindowUnit) => string = sizeParam,
): WindowSpec | null {
  if (!strict) return null;
  const lag = typeof strict[WINDOW_LAG_PARAM] === 'number' ? strict[WINDOW_LAG_PARAM] : 0;
  for (const unit of WINDOW_UNITS) {
    const size = strict[axis(unit)];
    if (typeof size === 'number' && Number.isFinite(size) && size > 0) return { size, lag, unit };
  }
  return null;
}

/** The slice axis of a group instance, if it authors one. */
export function sliceSpecFromStrict(
  strict: Record<string, number> | undefined,
): WindowSpec | null {
  return windowSpecFromStrict(strict, sliceSizeParam);
}

/**
 * `30s`, `30sl`, `20p`, `30sl@1` — the span, named. The `@lag` half appears only when
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
 * Parse a span written by {@link formatWindowSpec} — the frontend mirror of the Rust
 * `WindowSpec::parse`, and the grammar the `?windows=` query and a persisted exit
 * reason are both written in.
 *
 * `null` on anything malformed: a caller must not silently read an unrecognised
 * suffix as a bare number, which is how `30sl` would become a 30-SECOND window. A
 * bare number IS seconds, so a span written before the other bases existed — and a
 * pane preference saved back then — still parses to exactly what it meant.
 */
export function parseWindowSpec(raw: string): WindowSpec | null {
  const text = raw.trim();
  const at = text.indexOf('@');
  let head = text;
  let lag = 0;
  if (at >= 0) {
    lag = Number(text.slice(at + 1));
    if (!Number.isFinite(lag) || lag < 0) return null;
    head = text.slice(0, at);
  }
  // Longest suffix first, or `sl` parses as a seconds span with a stray `l`.
  let unit: WindowUnit = 'sec';
  let sizeText = head;
  for (const u of [...WINDOW_UNITS].sort((a, b) => unitSuffix(b).length - unitSuffix(a).length)) {
    if (head.endsWith(unitSuffix(u))) {
      unit = u;
      sizeText = head.slice(0, -unitSuffix(u).length);
      break;
    }
  }
  const size = Number(sizeText.trim());
  if (!Number.isFinite(size) || size <= 0) return null;
  return { size, lag, unit };
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
 * The scalar is `null` on a slot or print window (neither has seconds to report), so
 * a reader that only knows the scalar drops the window entirely rather than
 * mislabelling it — but every reader here should prefer the object when present.
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
