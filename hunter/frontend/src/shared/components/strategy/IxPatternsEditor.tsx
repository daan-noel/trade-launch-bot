import { useEffect, useMemo, useState } from 'react';

import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { CloseIcon, PlusIcon, SearchIcon, TrashIcon } from 'components/ui/icons';
import { IxLabelsInput } from 'components/ui/IxLabelsInput';
import { cn } from 'lib/cn';
import { formatIxLabelsText, parseIxLabelsText } from 'lib/ixLabels';

export interface IxPatternsEditorProps {
  /** Ordered label sequences — one row per volume-side ix structure. */
  patterns: string[][];
  onChange: (patterns: string[][]) => void;
  disabled?: boolean;
}

/** One wording for every "delete all patterns" confirm (editor + discovery cart). */
export function clearPrompt(patterns: string[][]): string {
  const n = patterns.filter((p) => p.length > 0).length;
  return `Delete all ${n} volume_ix_pattern${n === 1 ? '' : 's'}? Flow metrics stay NaN until you add one back.`;
}

/** Above this many rows the list gets a filter box — below it, scanning beats typing. */
const FILTER_THRESHOLD = 6;

/** `"Compute Budget: SetComputeUnitLimit"` becomes `"SetComputeUnitLimit"`.
 *
 *  The program half is what a sequence repeats most and varies least, so it is the
 *  half worth dropping to fit a row on one line. The full label stays one hover (row
 *  `title`) and one click (expand) away. */
function shortLabel(label: string): string {
  const i = label.lastIndexOf(': ');
  return i === -1 ? label : label.slice(i + 2);
}

/**
 * Registry-driven editor for `m_flow_ix.ix_patterns` (`string[][]`): each row is an
 * exact ordered ix-label sequence (same vocabulary as fingerprint `ix_labels`). Empty
 * list ⇒ fingerprint unconfigured for flow (metrics stay NaN).
 *
 * **One line per pattern, one open editor at a time.** A pattern is a 6-10 label
 * sequence and a real fingerprint carries 50-150 of them, so a textarea per row is
 * hundreds of lines of form — the list stops being scannable long before it stops
 * fitting. A collapsed row renders its sequence as one truncated line; expanding a
 * row is what shows the JSON.
 *
 * Every row stays MOUNTED whether collapsed, expanded, or filtered out — only its
 * editor is hidden. A row holds the draft text that has not parsed into labels yet,
 * so unmounting one discards whatever is half-typed in it the moment a filter
 * keystroke stops matching it.
 */
export function IxPatternsEditor({ patterns, onChange, disabled }: IxPatternsEditorProps) {
  const [open, setOpen] = useState<number | null>(null);
  const [query, setQuery] = useState('');

  const needle = query.trim().toLowerCase();
  const matches = useMemo(
    () => patterns.map((p) => needle === '' || p.some((l) => l.toLowerCase().includes(needle))),
    [patterns, needle],
  );
  const hitCount = matches.filter(Boolean).length;

  const remove = (i: number) => {
    onChange(patterns.filter((_, j) => j !== i));
    // The open row is identified by INDEX, so removing a row above it changes which
    // row that index names — follow the row rather than leaving the wrong one open.
    setOpen((cur) => (cur === null ? null : cur === i ? null : cur > i ? cur - 1 : cur));
  };

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        {!disabled && (
          <IconButton
            variant="success"
            size="sm"
            type="button"
            label="Add"
            onClick={() => {
              onChange([...patterns, []]);
              setOpen(patterns.length); // a blank row is only useful open
            }}
            title="Add pattern"
          >
            <PlusIcon />
          </IconButton>
        )}
        {patterns.length > FILTER_THRESHOLD && (
          <div className="relative min-w-0 flex-1">
            <SearchIcon className="pointer-events-none absolute left-1.5 top-1/2 h-3 w-3 -translate-y-1/2 text-text-dim/60" />
            <Input
              fieldSize="sm"
              className="pl-6"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={`Filter ${patterns.length} patterns by label...`}
              aria-label="Filter patterns"
            />
          </div>
        )}
        <span className="ml-auto shrink-0 text-[10px] tabular-nums text-text-dim/70">
          {needle === '' ? patterns.length : `${hitCount} / ${patterns.length}`}
        </span>
        {!disabled && patterns.length > 0 && (
          <IconButton
            variant="danger"
            size="sm"
            type="button"
            onClick={() => {
              // Only worth a prompt when there is authored content to lose.
              if (patterns.some((p) => p.length > 0) && !window.confirm(clearPrompt(patterns)))
                return;
              onChange([]);
              setOpen(null);
            }}
            title={`Delete all ${patterns.length} patterns`}
            aria-label="Delete all patterns"
          >
            <TrashIcon />
          </IconButton>
        )}
      </div>

      {patterns.length === 0 ? (
        <p className="text-[10px] text-text-dim/70">
          No patterns — flow metrics stay NaN until you add at least one structure.
        </p>
      ) : (
        <ul className="flex max-h-64 flex-col gap-1 overflow-y-auto pr-1">
          {patterns.map((p, i) => (
            <PatternRow
              key={i}
              index={i}
              labels={p}
              disabled={disabled}
              expanded={open === i}
              hidden={!matches[i]}
              onToggle={() => setOpen((cur) => (cur === i ? null : i))}
              onCommit={(labels) => {
                const next = patterns.slice();
                next[i] = labels;
                onChange(next);
              }}
              onRemove={disabled ? undefined : () => remove(i)}
            />
          ))}
        </ul>
      )}
      {needle !== '' && hitCount === 0 && (
        <p className="text-[10px] text-text-dim/70">No pattern carries that label.</p>
      )}
    </div>
  );
}

function PatternRow({
  index,
  labels,
  disabled,
  expanded,
  hidden,
  onToggle,
  onCommit,
  onRemove,
}: {
  index: number;
  labels: string[];
  disabled?: boolean;
  expanded: boolean;
  hidden: boolean;
  onToggle: () => void;
  onCommit: (labels: string[]) => void;
  onRemove?: () => void;
}) {
  const [text, setText] = useState(() => formatIxLabelsText(labels));
  const parsed = parseIxLabelsText(text);
  const labelsKey = JSON.stringify(labels);

  // Parent replaced this row (e.g. reuse-from-run) — resync the draft.
  useEffect(() => {
    setText(formatIxLabelsText(labels));
    // labelsKey is the stable identity; `labels` is read for formatting.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- labelsKey SSOT
  }, [labelsKey]);

  const summary = labels.length === 0 ? 'empty — add labels' : labels.map(shortLabel).join(' › ');

  return (
    <li
      className={cn(
        'rounded border bg-white/3',
        hidden && 'hidden',
        parsed.error ? 'border-danger/50' : expanded ? 'border-accent/40' : 'border-white/8',
      )}
    >
      <div className="flex items-center gap-1.5 px-1.5 py-1">
        <span className="w-5 shrink-0 text-right font-mono text-[9px] text-text-dim/60">
          {index + 1}
        </span>
        <button
          type="button"
          onClick={onToggle}
          title={labels.length > 0 ? labels.join('\n') : 'Add labels'}
          aria-expanded={expanded}
          className={cn(
            'min-w-0 flex-1 truncate text-left font-mono text-[10px] hover:text-text',
            labels.length === 0 ? 'italic text-text-dim/60' : 'text-text-mid',
          )}
        >
          {summary}
        </button>
        <span className="shrink-0 text-[9px] tabular-nums text-text-dim/60">{labels.length} ix</span>
        {onRemove && (
          <IconButton
            variant="danger"
            size="sm"
            type="button"
            onClick={onRemove}
            title="Remove pattern"
            aria-label={`Remove pattern ${index + 1}`}
          >
            <CloseIcon />
          </IconButton>
        )}
      </div>
      {/* Hidden, never unmounted — see the component doc: an unmounted row loses the
          draft text that has not parsed into labels yet. An unparseable row stays
          open whatever the toggle says, or its error has nowhere to show. */}
      <div className={cn('px-1.5 pb-1.5', !expanded && !parsed.error && 'hidden')}>
        <IxLabelsInput
          value={text}
          disabled={disabled}
          onValueChange={(v) => {
            setText(v);
            const { labels: next, error } = parseIxLabelsText(v);
            if (error) return;
            onCommit(next ?? []);
          }}
          error={parsed.error}
          placeholder={'[\n  "Pump.Fun: Buy"\n]'}
        />
      </div>
    </li>
  );
}
