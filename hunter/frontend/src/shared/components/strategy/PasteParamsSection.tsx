import { useRef, useState } from 'react';
import { cn } from 'lib/cn';
import { Textarea } from 'components/ui/Input';
import { Accordion } from 'components/ui/Accordion';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import {
  parseBlob,
  type ApplyResult,
  type ParamGroup,
  type RuleParamsBlob,
  type Strategy,
  type PasteMode,
} from 'lib/params';

/** Human label for a group called out in the "not included" warning. */
const GROUP_LABEL: Record<ParamGroup, string> = {
  fingerprint: 'token fingerprint',
  sizing: 'sizing',
  entry: 'entry',
  exit: 'exit',
  mode: 'mode',
};

interface Props {
  strategy: Strategy;
  live: boolean;
  /** Called when the user clicks Apply with a valid, matching blob.
   *  The modal owns the apply logic (it knows the typed form); this component
   *  just parses the JSON, validates strategy/version, and passes the blob up. */
  onApply: (blob: RuleParamsBlob, mode: PasteMode) => ApplyResult;
}

export function PasteParamsSection({ strategy, live, onApply }: Props) {
  const [text, setText] = useState('');
  const [mode, setMode] = useState<PasteMode>('merge');
  const [result, setResult] = useState<ApplyResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const isEmpty = text.trim().length === 0;

  // Validate + apply a JSON blob string. `m` defaults to the current mode state,
  // but callers that just changed the mode (see the Mode toggle below) pass the
  // new value explicitly — state hasn't re-rendered yet at that point.
  const applyText = (raw: string, m: PasteMode = mode) => {
    setError(null);
    setResult(null);
    const blob = parseBlob(raw);
    if (!blob) {
      setError('Invalid JSON — paste a blob copied from a rule or sweep combo');
      return;
    }
    if (blob.strategy !== strategy) {
      setError(`Strategy mismatch: blob is "${blob.strategy}", form is "${strategy}" — not applied`);
      return;
    }
    setResult(onApply(blob, m));
  };

  const handleApply = () => applyText(text);

  // Switching mode re-applies immediately if there's already a blob loaded, so
  // toggling merge/replace always reflects what would actually happen instead
  // of leaving a stale result on screen.
  const handleModeChange = (m: PasteMode) => {
    setMode(m);
    if (!isEmpty) applyText(text, m);
  };

  const handleClear = () => {
    setText('');
    setResult(null);
    setError(null);
    textareaRef.current?.focus();
  };

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

  // Pasting directly into the box applies immediately — the box only ever
  // holds one blob, so a paste replaces its contents rather than inserting at
  // the cursor. Makes keyboard paste (Ctrl/Cmd+V) a one-step round-trip too,
  // matching "From clipboard".
  const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const raw = e.clipboardData.getData('text');
    if (!raw.trim()) return;
    e.preventDefault();
    setText(raw);
    applyText(raw);
  };

  // Ctrl/Cmd+Enter applies without leaving the keyboard — handy after
  // hand-editing a pasted value.
  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      applyText(text);
    }
  };

  return (
    <Accordion
      padding="sm"
      defaultOpen={false}
      title={
        <span className="text-[11px] font-bold uppercase tracking-wider text-text-dim">
          ⎘ Paste params
        </span>
      }
    >
      <div className="flex flex-col gap-2">
        <div className="relative">
          <Textarea
            ref={textareaRef}
            autoResize
            rows={3}
            value={text}
            onChange={(e) => { setText(e.target.value); setResult(null); setError(null); }}
            onPaste={handlePaste}
            onKeyDown={handleKeyDown}
            className={cn(
              'max-h-72 overflow-y-auto border px-2.5 py-1.5 font-mono text-[11px] text-text focus:border-white/25',
              isEmpty
                ? 'min-h-20 border-dashed border-white/15 bg-white/2 hover:border-white/25'
                : 'border-solid border-white/10 bg-surface',
            )}
          />
          {isEmpty && (
            <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-1 px-6 text-center">
              <span className="text-base text-text-dim/50">⎘</span>
              <span className="text-[11px] font-medium text-text-dim/70">
                Paste JSON from "Copy params" (rule row ⎘ or sweep combo ⎘)
              </span>
              <span className="text-[10px] text-text-dim/40">or press Ctrl/Cmd+V</span>
            </div>
          )}
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-1.5">
            <span className="text-[10px] text-text-dim">Mode</span>
            <div className="flex rounded-lg border border-white/6 bg-white/3 p-0.5">
              {(['merge', 'replace'] as PasteMode[]).map((m) => (
                <button
                  key={m}
                  type="button"
                  onClick={() => handleModeChange(m)}
                  className={cn(
                    'w-24 rounded-md px-2.5 py-1 text-[11px] font-semibold capitalize transition-all duration-150',
                    mode === m ? 'bg-primary/12 text-primary shadow-sm' : 'text-text-dim hover:text-text',
                  )}
                >
                  {m}
                </button>
              ))}
            </div>
          </div>

          <div className="flex items-center gap-2">
            {!isEmpty && (
              <Button className='w-56' variant="ghost" size="md" onClick={handleClear}>
                ✕ Clear
              </Button>
            )}
            <Button className='w-56' variant="ghost" size="md" onClick={handleClipboard}>
              ⎘ Paste
            </Button>
            <Button className='w-56' variant="primary" size="md" onClick={handleApply} disabled={isEmpty}>
              Apply
            </Button>
          </div>
        </div>

        {live && (
          <p className="text-[10px] text-text-dim/70">
            Rule is live — only sizing params (buy amount, concurrency) will be applied.
          </p>
        )}
        {error && <p className="text-[10px] text-red">⚠ {error}</p>}
        {result && (
          <div className="flex flex-col gap-1.5">
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge variant="success" size="sm">✓ {result.applied} applied</Badge>
              {result.skipped > 0 && (
                <Badge variant="warning" size="sm">⚠ {result.skipped} skipped (frozen)</Badge>
              )}
              {result.dropped > 0 && (
                <Badge variant="danger" size="sm">✕ {result.dropped} dropped (unknown)</Badge>
              )}
            </div>
            {result.emptyGroups.length > 0 && (
              <p className="text-[10px] text-warning">
                not included in this blob: {result.emptyGroups.map((g) => GROUP_LABEL[g]).join(', ')}
                {' '}(fill in manually)
              </p>
            )}
          </div>
        )}
      </div>
    </Accordion>
  );
}
