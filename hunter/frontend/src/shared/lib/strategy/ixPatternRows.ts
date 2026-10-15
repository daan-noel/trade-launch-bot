import { coreKey, coreMarks, coreMarksFromText } from './ixCore';

/**
 * One `ix_shape` row as it is STORED — an ordered label sequence, optionally
 * pinned to the fee budget the sending client compiles. The twin of the engine's
 * `IxShapeRows::parse`.
 *
 * The stored array holds three row shapes:
 *
 * * `["A","B"]` — a bare label sequence: an exact row, matching any budget.
 * * `{"labels":["A","B"],"cu_limit":300000}` — an exact row narrowed to one
 *   client's preset. Only the fields present are pinned.
 * * `{"labels":["A","B"],"level":"core","side":"sell","marks":"CP T0 A0"}` — a core
 *   row: every variant of the build that keeps the labels' core.
 *
 * This module is for the jobs that need the whole row: authoring a pin, toggling
 * one from a trade, and matching a trade against the rows.
 */

/** The three fields a row may pin. Absent = wildcard on that field. */
export interface IxPatternFee {
  cu_limit?: number | null;
  cu_price?: number | null;
  tip_lamports?: number | null;
}

/** A stored `ix_patterns` row, fee and all.
 *
 *  `level: 'core'` (engine `IxShapeRows`, `"level": "core"`) matches every variant
 *  of the build that keeps its core ([coreLabels]) instead of the exact sequence; it
 *  may also pin the side and the extras (`marks`, engine `core_marks_text`). An exact
 *  row carries neither. */
export interface IxPatternRow extends IxPatternFee {
  labels: string[];
  level?: 'core';
  side?: 'buy' | 'sell';
  marks?: string;
}

export const FEE_FIELDS = ['cu_limit', 'cu_price', 'tip_lamports'] as const;
export type IxPatternFeeField = (typeof FEE_FIELDS)[number];

/** Sticky modifiers on a trades table: which of this tx's fee fields a click
 *  copies onto the staged row. All-false (the default) stages the ix structure
 *  only, even when the tx carries every field. */
export type IxPatternFeeMask = Partial<Record<IxPatternFeeField, boolean>>;

/** The three fields a trade may carry — the values a mask copies from. */
export type IxPatternFeeSource = Pick<IxPatternFee, IxPatternFeeField>;

/** The most compute units one Solana transaction may request; the chain rejects a
 *  larger request, so pinning past it can never match a landed trade. Mirrors the
 *  backend's `MAX_TX_COMPUTE_UNITS`. */
export const MAX_TX_COMPUTE_UNITS = 1_400_000;

/** Whether a row is core-level. */
export function rowIsCore(row: IxPatternRow): boolean {
  return row.level === 'core';
}

/** Parse one stored row. `null` when it is neither shape, or carries no labels —
 *  the same rows the backend's parser refuses. */
export function parseIxPatternRow(raw: unknown): IxPatternRow | null {
  if (Array.isArray(raw)) {
    return raw.every((x) => typeof x === 'string') ? { labels: raw as string[] } : null;
  }
  if (!raw || typeof raw !== 'object') return null;
  const obj = raw as Record<string, unknown>;
  const labels = obj.labels;
  if (!Array.isArray(labels) || !labels.every((x) => typeof x === 'string')) return null;
  const row: IxPatternRow = { labels: labels as string[] };
  for (const f of FEE_FIELDS) {
    const v = obj[f];
    if (typeof v === 'number' && Number.isInteger(v) && v >= 0) row[f] = v;
  }
  if (obj.level != null && obj.level !== 'exact' && obj.level !== 'core') return null;
  if (obj.level === 'core') {
    row.level = 'core';
    if (obj.side === 'buy' || obj.side === 'sell') row.side = obj.side;
    else if (obj.side != null) return null;
    if (typeof obj.marks === 'string') {
      if (coreMarksFromText(obj.marks) === null) return null;
      row.marks = obj.marks;
    } else if (obj.marks != null) return null;
  } else if (obj.side != null || obj.marks != null) return null;
  return row;
}

/** Every parseable row of a stored `ix_patterns` array. */
export function parseIxPatternRows(raw: unknown): IxPatternRow[] {
  if (!Array.isArray(raw)) return [];
  return raw.map(parseIxPatternRow).filter((r): r is IxPatternRow => r !== null);
}

/** Whether a row pins anything at all. */
export function rowPinsFee(row: IxPatternFee): boolean {
  return FEE_FIELDS.some((f) => row[f] != null);
}

/** A row back to its stored form.
 *
 *  An unpinned row serializes as the bare array it came in as — not as
 *  `{labels: [...]}`. `tags` is part of a fingerprint's ROW identity, so a
 *  save that rewrites every unpinned row into a new shape would rewrite identity
 *  for every fingerprint in the app on the next edit, for no behaviour change.
 */
export function serializeIxPatternRow(row: IxPatternRow): string[] | Record<string, unknown> {
  const labels = row.labels.map((l) => l.trim()).filter(Boolean);
  if (!rowPinsFee(row) && !rowIsCore(row)) return labels;
  const out: Record<string, unknown> = { labels };
  if (rowIsCore(row)) {
    out.level = 'core';
    if (row.side) out.side = row.side;
    if (row.marks != null) out.marks = row.marks;
  }
  for (const f of FEE_FIELDS) {
    if (row[f] != null) out[f] = row[f];
  }
  return out;
}

/** Serialize a list, dropping rows whose labels are all blank.
 *
 *  Accepts a bare label sequence in place of a row, because the two shapes are also
 *  what the STORED array holds: a caller holding either can write without first
 *  deciding which. */
export function serializeIxPatternRows(
  rows: (IxPatternRow | string[])[],
): (string[] | Record<string, unknown>)[] {
  return rows
    .map((r) => (Array.isArray(r) ? { labels: r } : r))
    .map((r) => ({ ...r, labels: r.labels.map((l) => l.trim()).filter(Boolean) }))
    .filter((r) => r.labels.length > 0)
    .map(serializeIxPatternRow);
}

/** Identity of a row's SHAPE — the key a pin is carried across a save by. */
export function patternKey(labels: readonly string[]): string {
  return JSON.stringify(labels.map((l) => l.trim()).filter(Boolean));
}

/** Identity of a stored row, pin and all. An unpinned row keys as its label
 *  array — the same string {@link patternKey} produces — so a structure-only
 *  click and a labels-only key still agree. A pinned row keys as the object
 *  form, so `{labels, cu_limit: 300000}` is a different list entry from the
 *  bare sequence. */
export function patternRowKey(row: IxPatternRow): string {
  return JSON.stringify(serializeIxPatternRow(row));
}

function cloneRow(row: IxPatternRow): IxPatternRow {
  const out: IxPatternRow = { labels: [...row.labels] };
  if (rowIsCore(row)) {
    out.level = 'core';
    if (row.side) out.side = row.side;
    if (row.marks != null) out.marks = row.marks;
  }
  for (const f of FEE_FIELDS) {
    if (row[f] != null) out[f] = row[f];
  }
  return out;
}

function isPinnableInt(v: unknown): v is number {
  return typeof v === 'number' && Number.isInteger(v) && v >= 0;
}

/** Copy the mask's ON fields off a trade. A checked field the tx does not
 *  carry is skipped — there is nothing to pin — so a cu_limit-only mask on a
 *  tx with no cu_limit stages the structure alone. A real `0` (the "no tip"
 *  reading) is copied; `null` is not. */
export function feeFromTrade(
  t: IxPatternFeeSource,
  mask: IxPatternFeeMask | null | undefined,
): IxPatternFee {
  const fee: IxPatternFee = {};
  if (!mask) return fee;
  for (const f of FEE_FIELDS) {
    if (!mask[f]) continue;
    const v = t[f];
    if (isPinnableInt(v)) fee[f] = v;
  }
  return fee;
}

/** Build the row a trades-table click writes: this tx's labels, plus whichever
 *  fee fields the mask copied. */
export function rowFromTrade(
  labels: readonly string[],
  t: IxPatternFeeSource,
  mask?: IxPatternFeeMask | null,
): IxPatternRow {
  const row: IxPatternRow = { labels: [...labels] };
  const fee = feeFromTrade(t, mask);
  for (const f of FEE_FIELDS) {
    if (fee[f] != null) row[f] = fee[f];
  }
  return row;
}

/** Whether any mask field is on — the click will try to copy those values. */
export function feeMaskActive(mask: IxPatternFeeMask | null | undefined): boolean {
  return !!mask && FEE_FIELDS.some((f) => mask[f]);
}

/**
 * Engine `FeeSpec::matches`: an absent pin is a wildcard on that field; a pin
 * requires the trade's reading to equal it. A missing reading never satisfies a
 * pin — same rule as the matcher, so a structure-only row still matches a tx
 * that carries every fee field.
 */
export function feeMatchesTrade(row: IxPatternFee, t: IxPatternFeeSource): boolean {
  for (const f of FEE_FIELDS) {
    if (row[f] == null) continue;
    if (t[f] !== row[f]) return false;
  }
  return true;
}

/** A trade as a row reads it: its budget, and its side for a core row's side pin. */
export type IxPatternTradeSource = IxPatternFeeSource & { side?: 'buy' | 'sell' | null };

/** Keys of a label array, memoized per array: the chart tests every row against every
 *  trade, so a tag of a thousand rows would otherwise re-serialize each pair. */
interface LabelKeys {
  exact: string;
  core: string;
  marks: number;
}
const labelKeysCache = new WeakMap<readonly string[], LabelKeys>();
function labelKeys(labels: readonly string[]): LabelKeys {
  let k = labelKeysCache.get(labels);
  if (!k) {
    k = { exact: patternKey(labels), core: coreKey(labels), marks: coreMarks(labels) };
    labelKeysCache.set(labels, k);
  }
  return k;
}

/** An exact row: labels exact-match AND the pins accept this tx's budget. A core row
 *  (engine `CoreSpec`): the cores match, the side and the extras match when pinned,
 *  and the pins accept the budget. */
export function rowMatchesTrade(
  row: IxPatternRow,
  labels: readonly string[],
  t: IxPatternTradeSource,
): boolean {
  const r = labelKeys(row.labels);
  const k = labelKeys(labels);
  if (!rowIsCore(row)) return r.exact === k.exact && feeMatchesTrade(row, t);
  if (r.core !== k.core) return false;
  if (row.side && t.side !== row.side) return false;
  if (row.marks != null && coreMarksFromText(row.marks) !== k.marks) return false;
  return feeMatchesTrade(row, t);
}

/** Whether one trade can match both rows: the test a listed row (a ranked shape, a
 *  budget, a staged row) is lit by under an armed one, so an arm lights its broader
 *  and its narrower rows alike. Two exact rows share a sequence; an exact row and a
 *  core row share the core and the core's marks; two core rows share the core and
 *  every side and marks both pin. A field both pin must hold one value. An exact
 *  row's side is open. */
export function rowsOverlap(a: IxPatternRow, b: IxPatternRow): boolean {
  for (const f of FEE_FIELDS) {
    if (a[f] != null && b[f] != null && a[f] !== b[f]) return false;
  }
  const ka = labelKeys(a.labels);
  const kb = labelKeys(b.labels);
  if (!rowIsCore(a) && !rowIsCore(b)) return ka.exact === kb.exact;
  if (ka.core !== kb.core) return false;
  if (a.side && b.side && a.side !== b.side) return false;
  // An exact row's marks are its labels'; a core row's are its pin, or open.
  const marks = (r: IxPatternRow, k: LabelKeys) =>
    rowIsCore(r) ? (r.marks != null ? coreMarksFromText(r.marks) : null) : k.marks;
  const ma = marks(a, ka);
  const mb = marks(b, kb);
  return ma == null || mb == null || ma === mb;
}

/** Whether any stored row accepts this trade — the engine's list match.
 *
 *  An unpinned row of this shape matches regardless of the trade's fee fields,
 *  which is why a structure-only checkbox stays selected when the pin strip is
 *  on and when the tx itself carries a budget. A pin-only list lights only the
 *  trades that satisfy that pin. */
export function anyRowMatchesTrade(
  rows: readonly IxPatternRow[],
  labels: readonly string[],
  t: IxPatternTradeSource,
): boolean {
  return rows.some((r) => rowMatchesTrade(r, labels, t));
}

/** Short readout of a row's pins, for a cart chip. Empty when unpinned. */
export function formatFeePins(row: IxPatternFee): string {
  const parts: string[] = [];
  if (row.cu_limit != null) parts.push(`cu ${row.cu_limit.toLocaleString()}`);
  if (row.cu_price != null) parts.push(`@ ${row.cu_price.toLocaleString()}`);
  if (row.tip_lamports != null) parts.push(`tip ${row.tip_lamports.toLocaleString()}`);
  return parts.join(' ');
}

/**
 * The family a row belongs to: an exact row by its whole sequence, a core row by
 * its core. Per family the list holds either one broad row or one-or-more narrow
 * rows - never both. A broad row already matches everything a narrow row of its
 * family does, so a narrow row beside it cannot narrow anything.
 */
export function rowFamilyKey(row: IxPatternRow): string {
  return `${rowIsCore(row) ? 'core' : 'exact'}|${patternKey(row.labels)}`;
}

/** Whether a row narrows its family: a fee pin, or a side or marks pin on a core row. */
export function rowNarrows(row: IxPatternRow): boolean {
  return rowPinsFee(row) || (rowIsCore(row) && (row.side != null || row.marks != null));
}

/** How a list holds `row`'s family: `broad` = its broad row is listed, `narrow` =
 *  only narrow rows of it are, `none` = nothing of it is. */
export type RowFamilyState = 'broad' | 'narrow' | 'none';

export function rowFamilyState(patterns: readonly IxPatternRow[], row: IxPatternRow): RowFamilyState {
  const fam = rowFamilyKey(row);
  let narrow = false;
  for (const p of patterns) {
    if (rowFamilyKey(p) !== fam) continue;
    if (!rowNarrows(p)) return 'broad';
    narrow = true;
  }
  return narrow ? 'narrow' : 'none';
}

/** Replaces every row of family `fam` with `insert`, sitting where the first of
 *  that family was. Empty `insert` deletes the family. Other rows keep their order. */
function spliceFamily(
  patterns: readonly IxPatternRow[],
  fam: string,
  insert: readonly IxPatternRow[],
): IxPatternRow[] {
  const out: IxPatternRow[] = [];
  let placed = false;
  for (const p of patterns) {
    if (rowFamilyKey(p) !== fam) {
      out.push(cloneRow(p));
      continue;
    }
    if (!placed) {
      for (const r of insert) out.push(cloneRow(r));
      placed = true;
    }
  }
  if (!placed) {
    for (const r of insert) out.push(cloneRow(r));
  }
  return out;
}

/**
 * Stage or unstage one row, keeping the per-family invariant ({@link rowFamilyKey}).
 * Exact rows and core rows follow the same rule; narrow = {@link rowNarrows}.
 *
 * * Broad, no broad row listed → widen (drop narrow rows of the family, one broad row).
 * * Broad, broad row listed → remove it.
 * * Narrow, broad row listed → narrow (drop the broad row, keep this one).
 * * Narrow, this row listed → remove it (other narrow rows of the family stay).
 * * Narrow, other narrow rows listed → add this one (OR).
 *
 * Empty labels are ignored.
 */
export function togglePatternRow(
  patterns: readonly IxPatternRow[],
  row: IxPatternRow,
): IxPatternRow[] {
  const labels = row.labels.map((l) => l.trim()).filter(Boolean);
  if (labels.length === 0) return patterns.map(cloneRow);
  const next: IxPatternRow = { ...cloneRow(row), labels };
  const fam = rowFamilyKey(next);
  const ofFam = patterns.filter((p) => rowFamilyKey(p) === fam);

  if (!rowNarrows(next)) {
    if (ofFam.some((p) => !rowNarrows(p))) return spliceFamily(patterns, fam, ofFam.filter(rowNarrows));
    return spliceFamily(patterns, fam, [next]);
  }

  const key = patternRowKey(next);
  if (ofFam.some((p) => patternRowKey(p) === key)) {
    return spliceFamily(
      patterns,
      fam,
      ofFam.filter((p) => rowNarrows(p) && patternRowKey(p) !== key),
    );
  }
  return spliceFamily(patterns, fam, [...ofFam.filter(rowNarrows), next]);
}

/** Stage each row unless the list already carries it: skipped when that row or
 *  its family's broad row is listed, otherwise one {@link togglePatternRow} add (a
 *  broad row widens a family that holds only narrow rows). */
export function addPatternRows(
  patterns: readonly IxPatternRow[],
  rows: readonly IxPatternRow[],
): IxPatternRow[] {
  let next = patterns.map(cloneRow);
  for (const row of rows) {
    const cleaned = { ...row, labels: row.labels.map((l) => l.trim()).filter(Boolean) };
    if (cleaned.labels.length === 0) continue;
    const key = patternRowKey(cleaned);
    if (rowFamilyState(next, cleaned) === 'broad' || next.some((r) => patternRowKey(r) === key)) continue;
    next = togglePatternRow(next, cleaned);
  }
  return next;
}

/** Drop exactly these rows. Other rows of their families stay - unstaging a broad
 *  row does not delete a narrow one. */
export function removePatternRows(
  patterns: readonly IxPatternRow[],
  rows: readonly IxPatternRow[],
): IxPatternRow[] {
  const drop = new Set(rows.map(patternRowKey));
  return patterns.filter((r) => !drop.has(patternRowKey(r))).map(cloneRow);
}