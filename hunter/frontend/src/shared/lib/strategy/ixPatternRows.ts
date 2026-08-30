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
 * in `registry` keep handing those callers `string[][]`. This module is for the two
 * jobs that need the whole row: authoring a pin, and **not destroying one** on a
 * save from a surface that cannot edit it.
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
export function patternKey(labels: string[]): string {
  return JSON.stringify(labels.map((l) => l.trim()).filter(Boolean));
}

/**
 * Re-attach the pins `prev` carried to a list of plain label sequences.
 *
 * This is what keeps a labels-only surface from silently deleting a budget: the
 * fingerprint form, the flow lens, the sweep config and the discovery cart all edit
 * `string[][]`, and without this any save from any of them would rewrite a pinned
 * row as a bare one — widening a rule's fire set with nothing on screen to say so.
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
