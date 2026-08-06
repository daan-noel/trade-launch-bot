// ix_labels text SSOT — pretty-printed JSON string array for edit/display,
// plus the Tokens/DataTable filter grammar (JSON ordered-exact vs text substring).
// Shared by FingerprintForm, FingerprintGroupPicker, creation-stats, and table filters.
// Paste also accepts newline- or comma-separated legacy text (no leading `[`).

export interface ParseIxLabelsResult {
  /** Parsed labels, or `null` when empty / no usable labels (no filter / no criterion). */
  labels: string[] | null;
  /** Set when non-empty text fails to parse as a string[]. */
  error: string | null;
}

/** Tokens / table column filter: JSON ordered-exact vs plain text substring-any. */
export type IxLabelFilter =
  | { kind: 'none' }
  | { kind: 'text'; needles: string[] }
  | { kind: 'json'; needles: string[] };

/** Placeholder + tooltip for DataTable columns that use {@link ixLabelsMatchFilter}. */
export const IX_LABELS_FILTER_PLACEHOLDER = '["Create","Buy"]  or  Buy';
export const IX_LABELS_FILTER_TITLE =
  'JSON array/object = ordered exact match; otherwise newline/comma list matches any label substring';

/**
 * **The one place that decides whether a label axis is configured.** Mirrors Rust
 * `hunter_engine::fingerprint::configured_labels`: an empty array is a second
 * spelling of "not set" — exactly like a `0` sentinel on a numeric axis — so it
 * collapses to `null` everywhere.
 *
 * Two readers disagreeing about this is not cosmetic: on the backend the engine
 * matcher turns "no criteria" into *matches nothing* while the creation-stats SQL
 * mirror turns it into *matches every token in the window*. Any UI that counts
 * criteria must reach the same verdict as `Fingerprint::has_any_criterion`.
 *
 * Note the contrast with a SOL axis VALUE, where `0` is a real bucket
 * (`[0, width)`) and only `null` drops the axis — don't fold the two rules.
 */
export function configuredIxLabels(
  labels: string[] | null | undefined,
): string[] | null {
  return labels != null && labels.length > 0 ? labels : null;
}

/** Serialize labels for the textarea / display (pretty JSON array). Empty ⇒ `""`. */
export function formatIxLabelsText(labels: string[] | null | undefined): string {
  const list = labels ?? [];
  if (list.length === 0) return '';
  return JSON.stringify(list, null, 2);
}

/**
 * Parse textarea / filter text into labels.
 * - empty ⇒ `{ labels: null, error: null }`
 * - JSON string array (primary) — pretty or compact
 * - else newline- or comma-separated legacy paste
 * Whitespace-only entries dropped; empty result after filter ⇒ `labels: null`.
 */
export function parseIxLabelsText(text: string): ParseIxLabelsResult {
  const t = text.trim();
  if (t === '') return { labels: null, error: null };

  if (t.startsWith('[')) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(t);
    } catch {
      return { labels: null, error: 'Invalid JSON' };
    }
    if (!Array.isArray(parsed) || !parsed.every((x) => typeof x === 'string')) {
      return {
        labels: null,
        error: 'Expected a JSON array of strings, e.g. ["Pump.Fun: Buy"]',
      };
    }
    const labels = (parsed as string[]).map((s) => s.trim()).filter((s) => s !== '');
    return { labels: labels.length > 0 ? labels : null, error: null };
  }

  // Legacy paste: one-per-line or comma-separated (no leading `[`).
  const parts = t.includes('\n')
    ? t.split('\n')
    : t.includes(',')
      ? t.split(',')
      : [t];
  const labels = parts.map((s) => s.trim()).filter((s) => s !== '');
  return { labels: labels.length > 0 ? labels : null, error: null };
}

/**
 * The action half of a decorated label — `"Pump.Fun: Create_v2"` → `"Create_v2"`.
 * Labels are `"<Program>: <Action>"`; an undecorated label passes through. Split
 * on the LAST `": "` so a program name containing a colon still resolves.
 */
export function ixLabelAction(label: string): string {
  const i = label.lastIndexOf(': ');
  return i < 0 ? label.trim() : label.slice(i + 2).trim();
}

/**
 * Actions in on-chain order — `"Create_v2 > Create > BuyExactSolIn"`.
 *
 * **A label set's identity is the sequence, never its length.** Two sets that
 * differ only in one action (`Buy` vs `BuyExactSolIn`) are different match
 * criteria that arm on different tokens, so any surface collapsing them to a
 * bare `Nix` count renders — and, via `fingerprintNameFromGroupKey`, *names* —
 * two distinct fingerprints identically. Use this wherever the full JSON
 * doesn't fit (tooltips, filter text).
 */
export function ixLabelsActions(labels: string[]): string {
  return labels.map(ixLabelAction).join(' > ');
}

/**
 * Count plus the trailing action — `"3ix:BuyExactSolIn"`. The compact form for
 * text-only surfaces (auto-name, chart series label, legend line) where a color
 * ribbon can't be drawn. The tail is the axis that varies in practice (the buy
 * variant); it is a *discriminator*, not an identity — the full sequence still
 * lives in the tooltip / `IxLabelsDisplay` beside it.
 */
export function ixLabelsCountTail(labels: string[]): string {
  const n = labels.length;
  const tail = n > 0 ? ixLabelAction(labels[n - 1]) : '';
  return tail === '' ? `${n}ix` : `${n}ix:${tail}`;
}

/**
 * Column / Tokens filter grammar (mirrors hunter-core `ix_label` SQL):
 * - starts with `[`/`{` and parses as a JSON array (or `{ instructions: [...] }`)
 *   → ordered exact match (case-insensitive)
 * - otherwise → split on newline/comma; any needle is a substring of any label
 */
export function parseIxLabelFilter(raw: string): IxLabelFilter {
  const trimmed = raw.trim();
  if (!trimmed) return { kind: 'none' };

  if (trimmed.startsWith('[') || trimmed.startsWith('{')) {
    try {
      const parsed = JSON.parse(trimmed) as unknown;
      const arr = Array.isArray(parsed)
        ? parsed
        : (parsed as { instructions?: unknown[] })?.instructions;
      if (Array.isArray(arr)) {
        const needles = arr.map((v) => String(v).trim()).filter(Boolean);
        if (needles.length > 0) return { kind: 'json', needles };
      }
    } catch {
      /* fall through to text mode */
    }
  }

  const needles = trimmed
    .split(/[\n,]/)
    .map((s) => s.trim().toLowerCase())
    .filter(Boolean);
  return needles.length > 0 ? { kind: 'text', needles } : { kind: 'none' };
}

/** True when `raw` parses as the JSON ordered-exact branch (not text fallback). */
export function isIxLabelJsonFilter(raw: string): boolean {
  return parseIxLabelFilter(raw).kind === 'json';
}

function ixLabelsMatchJson(needles: string[], labels: string[]): boolean {
  const want = needles.map((n) => n.toLowerCase());
  const have = labels.map((l) => l.toLowerCase());
  if (want.length !== have.length) return false;
  return want.every((n, i) => have[i] === n);
}

function ixLabelsMatchText(needles: string[], labels: string[]): boolean {
  const have = labels.map((l) => l.toLowerCase());
  return needles.some((n) => have.some((l) => l.includes(n)));
}

/** Apply {@link parseIxLabelFilter} against a row's label list. Empty filter ⇒ true. */
export function ixLabelsMatchFilter(
  labels: string[] | null | undefined,
  raw: string,
): boolean {
  const filter = parseIxLabelFilter(raw);
  if (filter.kind === 'none') return true;
  const list = labels ?? [];
  return filter.kind === 'json'
    ? ixLabelsMatchJson(filter.needles, list)
    : ixLabelsMatchText(filter.needles, list);
}
