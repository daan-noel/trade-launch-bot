// ix_labels text SSOT — pretty-printed JSON string array for edit/display.
// Shared by FingerprintForm, FingerprintGroupPicker, and creation-stats.
// Paste also accepts newline- or comma-separated legacy text (no leading `[`).

export interface ParseIxLabelsResult {
  /** Parsed labels, or `null` when empty / no usable labels (no filter / no criterion). */
  labels: string[] | null;
  /** Set when non-empty text fails to parse as a string[]. */
  error: string | null;
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
