import { useMemo, useState } from 'react';
import { Button } from 'components/ui/Button';
import { cn } from 'lib/cn';

/**
 * Paste-a-set-of-mints filter box. Splits the pasted text on whitespace/commas/
 * newlines, base58-validates, dedups, and caps the set, then emits the normalized
 * mint list up to {@link TokenTable} (server mode) which folds it into the request
 * as an `in` op on the `mint` column (ANDs with the table's existing filters).
 *
 * Deliberately presentational + local: it owns only the textarea + parse state and
 * emits a `string[]` (empty = filter off) on Apply/Clear. The cap mirrors the
 * backend `trading_core::api::table_query::MAX_FILTER_IN_VALUES` so the two can't
 * drift; over-cap and invalid entries are surfaced as a "dropped N" note (no silent
 * truncation, per the data-scale guardrails).
 */

/** Keep in lock-step with `trading_core::api::table_query::MAX_FILTER_IN_VALUES`. */
const MAX_MINT_SET = 500;

/** Solana base58 address: base58 alphabet, 32–44 chars (mint pubkeys are 43–44). */
const BASE58_RE = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;

interface ParsedMints {
  /** The normalized, deduped, capped mint list. */
  mints: string[];
  /** Distinct valid mints before the cap (so we can report how many were dropped). */
  validCount: number;
  /** Entries that failed base58 validation. */
  invalid: number;
  /** Valid mints beyond the cap that were dropped. */
  dropped: number;
}

function parseMints(text: string): ParsedMints {
  const tokens = text.split(/[\s,]+/).filter(Boolean);
  const seen = new Set<string>();
  const valid: string[] = [];
  let invalid = 0;
  for (const t of tokens) {
    if (!BASE58_RE.test(t)) {
      invalid += 1;
      continue;
    }
    if (seen.has(t)) continue;
    seen.add(t);
    valid.push(t);
  }
  return {
    mints: valid.slice(0, MAX_MINT_SET),
    validCount: valid.length,
    invalid,
    dropped: Math.max(0, valid.length - MAX_MINT_SET),
  };
}

interface MintSetInputProps {
  /** Emits the normalized mint list on Apply, or `[]` on Clear (filter off). */
  onChange: (mints: string[]) => void;
  /** Count of mints currently applied (for the toggle badge). */
  appliedCount: number;
  className?: string;
}

export function MintSetInput({ onChange, appliedCount, className }: MintSetInputProps) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState('');
  const parsed = useMemo(() => parseMints(text), [text]);

  return (
    <div className={cn('mb-2', className)}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'rounded-md border border-white/8 bg-white/4 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wider text-text-dim transition hover:text-text',
          (open || appliedCount > 0) && 'border-primary/35 bg-primary/12 text-primary',
        )}
      >
        {appliedCount > 0 ? `Mint set (${appliedCount})` : 'Mint set'}
      </button>

      {open && (
        <div className="mt-2 flex flex-col gap-2 rounded-lg border border-white/7 bg-white/2 p-3">
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            rows={4}
            placeholder="Paste mints — whitespace, comma, or newline separated"
            className="w-full resize-y rounded-md border border-white/8 bg-bg-panel px-2 py-1.5 font-mono text-[11px] text-text focus:border-primary focus:outline-none"
          />
          <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-dim">
            <span>
              <span className="font-mono text-text">{parsed.mints.length}</span> valid mint
              {parsed.mints.length === 1 ? '' : 's'}
            </span>
            {parsed.invalid > 0 && (
              <span className="text-warning">· {parsed.invalid} invalid dropped</span>
            )}
            {parsed.dropped > 0 && (
              <span className="text-warning">· {parsed.dropped} over the {MAX_MINT_SET} cap dropped</span>
            )}
            <span className="flex-1" />
            <Button
              variant="ghost"
              size="xs"
              disabled={!text.trim() && appliedCount === 0}
              onClick={() => {
                setText('');
                onChange([]);
              }}
            >
              Clear
            </Button>
            <Button
              variant="primary"
              size="xs"
              disabled={parsed.mints.length === 0}
              onClick={() => onChange(parsed.mints)}
            >
              Apply
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
