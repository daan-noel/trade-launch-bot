import { useState } from 'react';
import { cn } from 'lib/cn';
import {
  parseParamsBlob,
  type ApplyResult,
  type RuleParamsBlob,
  type Strategy,
  type PasteMode,
} from 'lib/ruleParams';

interface Props {
  strategy: Strategy;
  live: boolean;
  /** Called when the user clicks Apply with a valid, matching blob.
   *  The modal owns the apply logic (it knows the typed form); this component
   *  just parses the JSON, validates strategy/version, and passes the blob up. */
  onApply: (blob: RuleParamsBlob, mode: PasteMode) => ApplyResult;
}

export function PasteParamsSection({ strategy, live, onApply }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [text, setText] = useState('');
  const [mode, setMode] = useState<PasteMode>('merge');
  const [result, setResult] = useState<ApplyResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Validate + apply a JSON blob string. Shared by the textarea's Apply button
  // and the one-click "From clipboard" button.
  const applyText = (raw: string) => {
    setError(null);
    setResult(null);
    const blob = parseParamsBlob(raw);
    if (!blob) {
      setError('Invalid JSON — paste a blob copied from a rule or sweep combo');
      return;
    }
    if (blob.strategy !== strategy) {
      setError(`Strategy mismatch: blob is "${blob.strategy}", form is "${strategy}" — not applied`);
      return;
    }
    setResult(onApply(blob, mode));
  };

  const handleApply = () => applyText(text);

  // Read the clipboard and apply in one click. Mirrors the ⎘ Copy buttons, so
  // copy→paste is a two-click round-trip without touching the textarea. Fills
  // the textarea too, so a failed parse is visible/editable.
  const handleClipboard = async () => {
    setError(null);
    setResult(null);
    try {
      const raw = await navigator.clipboard.readText();
      setText(raw);
      applyText(raw);
    } catch {
      setError('Clipboard read blocked — paste into the box above and click Apply');
    }
  };

  return (
    <div className="rounded-lg border border-white/10 bg-white/3 p-2.5">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-1.5 text-left text-[10px] font-bold uppercase tracking-wider text-text-dim hover:text-text"
      >
        <span className="text-[8px]">{expanded ? '▾' : '▸'}</span>
        Paste params
      </button>

      {expanded && (
        <div className="mt-2 flex flex-col gap-2">
          <textarea
            rows={3}
            value={text}
            onChange={(e) => { setText(e.target.value); setResult(null); setError(null); }}
            placeholder='Paste JSON from "Copy params" (rule row ⎘ or sweep combo ⎘)…'
            className="w-full resize-none rounded-md border border-white/10 bg-surface px-2.5 py-1.5 font-mono text-[11px] text-text placeholder:text-text-dim/50 focus:border-white/25 focus:outline-none"
          />

          <div className="flex items-center gap-2">
            <span className="text-[10px] text-text-dim">Mode</span>
            {(['merge', 'replace'] as PasteMode[]).map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => setMode(m)}
                className={cn(
                  'rounded border px-2 py-0.5 text-[10px] font-semibold capitalize transition-colors',
                  mode === m
                    ? 'border-primary/50 bg-primary/15 text-primary'
                    : 'border-white/10 text-text-dim hover:border-white/20 hover:text-text',
                )}
              >
                {m}
              </button>
            ))}
            <div className="flex-1" />
            <button
              type="button"
              onClick={handleClipboard}
              className="rounded border border-white/10 px-3 py-1 text-[11px] font-semibold text-text-dim hover:border-white/20 hover:text-text"
            >
              ⎘ From clipboard
            </button>
            <button
              type="button"
              onClick={handleApply}
              disabled={!text.trim()}
              className="rounded bg-primary/15 px-3 py-1 text-[11px] font-semibold text-primary hover:bg-primary/25 disabled:cursor-not-allowed disabled:opacity-40"
            >
              Apply
            </button>
          </div>

          {live && (
            <p className="text-[10px] text-text-dim/70">
              Rule is live — only sizing params (buy amount, concurrency) will be applied.
            </p>
          )}
          {error && <p className="text-[10px] text-red">{error}</p>}
          {result && (
            <p className="text-[10px] text-text-dim">
              <span className="text-green">applied {result.applied}</span>
              {result.skipped > 0 && (
                <> · <span className="text-warning">skipped {result.skipped} (frozen)</span></>
              )}
              {result.dropped > 0 && (
                <> · <span className="text-red/80">dropped {result.dropped} (unknown)</span></>
              )}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
