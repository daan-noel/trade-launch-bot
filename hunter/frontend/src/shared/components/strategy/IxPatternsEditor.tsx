import { useEffect, useState } from 'react';

import { IconButton } from 'components/ui/IconButton';
import { CloseIcon, PlusIcon, TrashIcon } from 'components/ui/icons';
import { IxLabelsInput } from 'components/ui/IxLabelsInput';
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

/**
 * Registry-driven editor for `m_flow_ix.ix_patterns` (`string[][]`):
 * each row is an exact ordered ix-label sequence (same vocabulary as fingerprint
 * `ix_labels`). Empty list ⇒ fingerprint unconfigured for flow (metrics stay NaN).
 */
export function IxPatternsEditor({
  patterns,
  onChange,
  disabled,
}: IxPatternsEditorProps) {
  return (
    <div className="flex flex-col gap-2">
      {patterns.length === 0 && (
        <p className="text-[10px] text-text-dim/70">
          No patterns — flow metrics stay NaN until you add at least one structure.
        </p>
      )}
      {patterns.map((p, i) => (
        <PatternRow
          key={i}
          labels={p}
          disabled={disabled}
          onCommit={(labels) => {
            const next = patterns.slice();
            next[i] = labels;
            onChange(next);
          }}
          onRemove={disabled ? undefined : () => onChange(patterns.filter((_, j) => j !== i))}
        />
      ))}
      {!disabled && (
        <div className="flex items-center gap-2">
          <IconButton
            variant="success"
            size="md"
            type="button"
            onClick={() => onChange([...patterns, []])}
            title="Add pattern"
            aria-label="Add pattern"
          >
            <PlusIcon />
          </IconButton>
          {patterns.length > 0 && (
            <IconButton
              variant="danger"
              size="md"
              type="button"
              label={`Delete all (${patterns.length})`}
              onClick={() => {
                // Only worth a prompt when there is authored content to lose.
                if (patterns.some((p) => p.length > 0) && !window.confirm(clearPrompt(patterns)))
                  return;
                onChange([]);
              }}
              title="Delete all patterns"
            >
              <TrashIcon />
            </IconButton>
          )}
        </div>
      )}
    </div>
  );
}

function PatternRow({
  labels,
  disabled,
  onCommit,
  onRemove,
}: {
  labels: string[];
  disabled?: boolean;
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

  return (
    <div className="flex items-start gap-1">
      <div className="min-w-0 flex-1">
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
      {onRemove && (
        <IconButton
          variant="danger"
          size="md"
          type="button"
          onClick={onRemove}
          title="Remove pattern"
          aria-label="Remove pattern"
        >
          <CloseIcon />
        </IconButton>
      )}
    </div>
  );
}
