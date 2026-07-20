import { useEffect, useState } from 'react';

import { Button } from 'components/ui/Button';
import { IxLabelsInput } from 'components/ui/IxLabelsInput';
import { formatIxLabelsText, parseIxLabelsText } from 'lib/ixLabels';

export interface VolumeIxPatternsEditorProps {
  /** Ordered label sequences — one row per volume-side ix structure. */
  patterns: string[][];
  onChange: (patterns: string[][]) => void;
  disabled?: boolean;
}

/**
 * Registry-driven editor for `m_flow_split.volume_ix_patterns` (`string[][]`):
 * each row is an exact ordered ix-label sequence (same vocabulary as fingerprint
 * `ix_labels`). Empty list ⇒ fingerprint unconfigured for flow (metrics stay NaN).
 */
export function VolumeIxPatternsEditor({
  patterns,
  onChange,
  disabled,
}: VolumeIxPatternsEditorProps) {
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
        <Button variant="ghost" size="sm" type="button" onClick={() => onChange([...patterns, []])}>
          + pattern
        </Button>
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
          placeholder='["create", "buy"]'
        />
      </div>
      {onRemove && (
        <Button variant="ghost" size="sm" type="button" onClick={onRemove} title="Remove pattern">
          ×
        </Button>
      )}
    </div>
  );
}
