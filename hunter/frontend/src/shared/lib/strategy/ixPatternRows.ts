/**
 * One `ix_patterns` row as it is STORED — an ordered label sequence, optionally
 * pinned to the fee budget the sending client compiles.
 *
 * The stored array holds two row shapes and both are current:
 *
 * * `["A","B"]` — a bare label sequence, matching any budget. Every list written
 *   before fee capture is entirely this form.
 * * `{"labels":["A","B"],"cu_limit":300000}` — the same shape narrowed to one
 *   client's preset. Only the fields present are pinned.
 *
 * Nearly every surface in the app wants only the labels (the chart's flow
 * classification, the lens, the fingerprint chips, the sweep axes), so the readers
 * in `registry` keep handing those callers `string[][]`. This module is for the jobs
 * that need the whole row: authoring a pin, toggling one from a trade, and **not
 * destroying one** on a save from a surface that cannot edit it.
 */

/** The three fields a row may pin. Absent = wildcard on that field. */
export interface IxPatternFee {
  cu_limit?: number | null;
  cu_price?: number | null;
  tip_lamports?: number | null;
}

/** A stored `ix_patterns` row, fee and all. */
export interface IxPatternRow extends IxPatternFee {
  labels: string[];
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
 *  `{labels: [...]}`. `metric_config` is part of a fingerprint's ROW identity, so a
 *  save that rewrites every unpinned row into a new shape would rewrite identity
 *  for every fingerprint in the app on the next edit, for no behaviour change.
 */
export function serializeIxPatternRow(row: IxPatternRow): string[] | Record<string, unknown> {
  const labels = row.labels.map((l) => l.trim()).filter(Boolean);
  if (!rowPinsFee(row)) return labels;
  const out: Record<string, unknown> = { labels };
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

/** Labels exact-match AND the row's pins accept this tx's budget. */
export function rowMatchesTrade(
  row: IxPatternRow,
  labels: readonly string[],
  t: IxPatternFeeSource,
): boolean {
  return patternKey(row.labels) === patternKey(labels) && feeMatchesTrade(row, t);
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
  t: IxPatternFeeSource,
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
 * Add or remove one exact row (labels + pins). An unpinned row and a pinned
 * row of the same shape are different entries: toggling one leaves the other.
 * Empty labels are ignored. Surviving rows keep their order.
 */
export function togglePatternRow(
  patterns: readonly IxPatternRow[],
  row: IxPatternRow,
): IxPatternRow[] {
  const labels = row.labels.map((l) => l.trim()).filter(Boolean);
  if (labels.length === 0) return patterns.map(cloneRow);
  const next: IxPatternRow = { ...cloneRow(row), labels };
  const key = patternRowKey(next);
  const kept = patterns.filter((p) => patternRowKey(p) !== key);
  if (kept.length !== patterns.length) return kept.map(cloneRow);
  return [...patterns.map(cloneRow), next];
}

/** Append each labels sequence as an unpinned row, skipping ones already in
 *  the list as unpinned. Pinned rows of the same shape are left alone. */
export function addUnpinnedPatterns(
  patterns: readonly IxPatternRow[],
  labelsList: readonly (readonly string[])[],
): IxPatternRow[] {
  const next = patterns.map(cloneRow);
  const have = new Set(
    next.filter((r) => !rowPinsFee(r)).map((r) => patternRowKey(r)),
  );
  for (const labels of labelsList) {
    const row: IxPatternRow = { labels: labels.map((l) => l.trim()).filter(Boolean) };
    if (row.labels.length === 0) continue;
    const key = patternRowKey(row);
    if (have.has(key)) continue;
    have.add(key);
    next.push(row);
  }
  return next;
}

/** Drop unpinned rows whose labels are in `labelsList`. Pinned rows of those
 *  shapes stay — unstaging a catch-all does not delete a fee-narrowed entry. */
export function removeUnpinnedPatterns(
  patterns: readonly IxPatternRow[],
  labelsList: readonly (readonly string[])[],
): IxPatternRow[] {
  const drop = new Set(labelsList.map((l) => patternKey([...l])));
  return patterns.filter((r) => rowPinsFee(r) || !drop.has(patternKey(r.labels))).map(cloneRow);
}

/**
 * Re-attach the pins `prev` carried to a list of plain label sequences.
 *
 * This is what keeps a labels-only surface from silently deleting a budget: the
 * fingerprint form, the flow lens and the sweep config all edit `string[][]`, and
 * without this any save from any of them would rewrite a pinned row as a bare one
 * — widening a rule's fire set with nothing on screen to say so. Discovery's cart
 * edits whole rows, so it writes them outright rather than through this.
 *
 * Keyed by the label sequence, because that is the only thing a labels-only surface
 * can have preserved. A shape the surface deleted loses its pin with it (correct —
 * the row is gone); a shape it added arrives unpinned (correct — it is new).
 *
 * A shape carrying SEVERAL pinned rows keeps all of them, expanding to one row per
 * pin. That is what a preset menu looks like, and collapsing it to the first pin
 * would quietly narrow the list.
 */
export function withPreservedFees(patterns: string[][], prev: unknown): IxPatternRow[] {
  const byShape = new Map<string, IxPatternRow[]>();
  for (const row of parseIxPatternRows(prev)) {
    if (!rowPinsFee(row)) continue;
    const key = patternKey(row.labels);
    byShape.set(key, [...(byShape.get(key) ?? []), row]);
  }
  return patterns.flatMap((labels) => {
    const pinned = byShape.get(patternKey(labels));
    return pinned && pinned.length > 0 ? pinned : [{ labels }];
  });
}
