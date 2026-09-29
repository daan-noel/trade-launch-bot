// ix_labels text SSOT — pretty-printed JSON string array for edit/display,
// plus the Tokens/DataTable filter grammar (JSON ordered-exact vs text substring).
// Shared by FingerprintForm, FingerprintGroupPicker, creation-stats, and table filters.
// Paste also accepts newline- or comma-separated legacy text (no leading `[`).

/**
 * Collect a persisted `ix_labels` value into an ordered label list — the TS mirror
 * of Rust `hunter_engine::grouping::normalize_labels`, preserving exact on-chain
 * order and count (no sort, no dedup).
 *
 * The column accepts **two** shapes and always has: the bare array `["A","B"]` and
 * the object wrapper `{"instructions":["A","B"]}` (see Rust
 * `storage::ix_labels_sql`). A reader that understands only the array form counts
 * every object-shaped row as having no labels, which never surfaces as an error —
 * "this trade has no labels" is a legal state — it just silently classifies the
 * trade organic and stages an empty volume pattern. Anything unrecognized ⇒ `[]`.
 */
export function normalizeIxLabels(raw: unknown): string[] {
  const arr = Array.isArray(raw)
    ? raw
    : (raw as { instructions?: unknown } | null)?.instructions;
  if (!Array.isArray(arr)) return [];
  return arr.filter((x): x is string => typeof x === 'string');
}

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
 * bare `Nix` count renders — and, via `fingerprintAutoName`, *names* —
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
 * Column / Tokens filter grammar (mirrors hunter-core `api/ix_label_filter.rs`):
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

/** Boilerplate instructions every build carries, as 1-2 letter codes, each with
 *  the words the legend spells it out in. */
const IX_ABBREV: Readonly<Record<string, readonly [code: string, meaning: string]>> = {
  'Compute Budget: SetComputeUnitLimit': ['CL', 'compute limit'],
  'Compute Budget: SetComputeUnitPrice': ['CP', 'compute price'],
  'Associated Token: Create': ['A', 'token account create'],
  'Associated Token: CreateIdempotent': ['A+', 'token account create (idempotent)'],
  'System Program: Transfer': ['T', 'SOL transfer'],
  'System Program: AdvanceNonceAccount': ['N', 'nonce'],
  'System Program: CreateAccount': ['NA', 'new account'],
  'System Program: CreateAccountWithSeed': ['SA', 'seeded account'],
  'Token Program: CloseAccount': ['X', 'close account'],
  'Token Program: SyncNative': ['SN', 'sync native'],
  'Memo Program: Memo': ['M', 'memo'],
};

/** Leading characters an unnamed program's id keeps (`6Vo3`). */
const PROGRAM_ID_CHARS = 4;

/** Capitals of a CamelCase word: `BuyExactSolIn` -> `BESI`, `Buy` -> `B`. */
const initials = (word: string) => word.match(/[A-Z0-9]/g)?.join('') || word.slice(0, 2);

/** `Unknown (6Vo3245e)` -> `6Vo3`, `Pump.Fun` -> `PF`, `Axiom Trade` -> `AT`,
 *  a one-word name its first two letters (`Bundler` -> `Bu`). */
function abbrevProgram(program: string): string {
  const unknown = /^Unknown \((.+)\)$/.exec(program);
  if (unknown) return unknown[1].slice(0, PROGRAM_ID_CHARS);
  const words = program.split(/[ .]+/).filter(Boolean);
  return words.length > 1 ? words.map((w) => w[0].toUpperCase()).join('') : program.slice(0, 2);
}

/** One abbreviated instruction. `setup` = boilerplate (compute, accounts,
 *  transfers): drawn dim so the program actions stand out. */
export interface IxAbbrevPart {
  code: string;
  setup: boolean;
}

/** One label short: a fixed code for boilerplate, else `Program:Action` initials. */
function abbrevLabel(label: string): IxAbbrevPart {
  const fixed = IX_ABBREV[label];
  if (fixed) return { code: fixed[0], setup: true };
  const i = label.lastIndexOf(': ');
  return {
    code: i === -1 ? label : `${abbrevProgram(label.slice(0, i))}:${initials(label.slice(i + 2))}`,
    setup: false,
  };
}

/** What the codes mean, for a column header or legend - built from the same table. */
export const IX_ABBREV_LEGEND = [
  ...Object.values(IX_ABBREV).map(([code, meaning]) => `${code} = ${meaning}`),
  'Program:Action by initials, e.g. PF:B = Pump.Fun Buy, 6Vo3:B = unnamed program 6Vo3... Buy',
  '×N = the same instruction N times in a row',
].join('\n');

/** Separator between abbreviated instructions: an arrow reads as "then". */
export const IX_ABBREV_SEP = '→';

/**
 * **The one-line form of an exact ix sequence**, in on-chain order and as short as
 * it can read: boilerplate as 1-2 letter codes (`CL` compute limit, `CP` compute
 * price, `A+` token account, `T` transfer, ... - see {@link IX_ABBREV_LEGEND}), any
 * other label as `Program:Action` initials (`PF:B` Pump.Fun Buy, `6Vo3:B` an unnamed
 * program by its id's first 4 characters), a run of one label as `×N`. Parts, so a
 * renderer can dim the setup codes (`IxLabelsDisplay` `compact`).
 */
export function abbreviateIxLabelParts(labels: readonly string[]): IxAbbrevPart[] {
  const out: IxAbbrevPart[] = [];
  let prev: IxAbbrevPart | null = null;
  let run = 0;
  const flush = () => {
    if (prev) out.push(run > 1 ? { ...prev, code: `${prev.code}×${run}` } : prev);
  };
  for (const l of labels) {
    const a = abbrevLabel(l);
    if (prev && a.code === prev.code) {
      run += 1;
      continue;
    }
    flush();
    prev = a;
    run = 1;
  }
  flush();
  return out;
}

/** The same as text: `CL → CP → A+ → 6Vo3:B → T×2`. */
export function abbreviateIxLabels(labels: readonly string[]): string {
  return abbreviateIxLabelParts(labels)
    .map((p) => p.code)
    .join(` ${IX_ABBREV_SEP} `);
}
