import { useMemo, useState, type CSSProperties, type MouseEvent } from 'react';
import { abbreviateIxLabelParts, formatIxLabelsText, IX_ABBREV_SEP } from 'lib/ixLabels';
import { cn } from 'lib/cn';

export interface IxLabelsDisplayProps {
  labels: string[];
  /** Click copies the pretty-printed JSON (default false). */
  copyJson?: boolean;
  /** Cap height for dense table cells; overflow scrolls. CSS length, e.g. `4.5rem`. */
  maxHeight?: string;
  /** Shown when `labels` is empty. Omit to render nothing. */
  empty?: string;
  /** One-line mode for a narrow column: the sequence abbreviated
   *  (`abbreviateIxLabelParts`) on a single truncated line, the full list on hover,
   *  click still copies. */
  compact?: boolean;
  className?: string;
  style?: CSSProperties;
}

/**
 * Pretty-printed JSON array of instruction labels (on-chain order) — plain
 * left-aligned mono text with indent, no badge/chip chrome.
 */
export function IxLabelsDisplay({
  labels,
  copyJson = false,
  maxHeight,
  empty,
  compact = false,
  className,
  style,
}: IxLabelsDisplayProps) {
  const [copied, setCopied] = useState(false);
  const json = useMemo(() => formatIxLabelsText(labels), [labels]);

  if (labels.length === 0) {
    return empty != null ? <span className={className}>{empty}</span> : null;
  }

  const copy = async (e: MouseEvent) => {
    if (!copyJson) return;
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(json);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  if (compact) {
    return (
      <span
        onClick={copyJson ? copy : undefined}
        title={`${json}${copyJson ? `\n\n${copied ? 'Copied!' : 'Click to copy JSON'}` : ''}`}
        className={cn(
          'block truncate font-mono text-[11px] text-text-mid',
          copyJson && 'cursor-pointer',
          copied && 'text-primary',
          className,
        )}
        style={style}
      >
        <IxAbbrevLine labels={labels} />
      </span>
    );
  }

  return (
    <pre
      onClick={copyJson ? copy : undefined}
      title={copyJson ? (copied ? 'Copied!' : 'Click to copy JSON') : undefined}
      className={cn(
        'm-0 block whitespace-pre text-left font-mono text-[11px] leading-relaxed text-text-mid',
        copyJson && 'cursor-pointer',
        copied && 'text-primary',
        maxHeight && 'overflow-y-auto',
        className,
      )}
      style={{
        ...style,
        ...(maxHeight ? { maxHeight } : undefined),
      }}
    >
      {json}
    </pre>
  );
}

/**
 * An ix sequence as its abbreviation line: dim arrows between instructions, setup
 * codes (compute, accounts, transfers) dim and the program actions bright, so the
 * eye lands on what the transaction does. Inline, no chrome - the caller owns the
 * box, truncation and hover.
 */
export function IxAbbrevLine({ labels }: { labels: readonly string[] }) {
  const parts = useMemo(() => abbreviateIxLabelParts(labels), [labels]);
  return (
    <>
      {parts.map((p, i) => (
        <span key={i}>
          {i > 0 && <span className="px-1 text-text-dim/50">{IX_ABBREV_SEP}</span>}
          <span className={p.setup ? 'text-text-dim' : 'font-semibold text-text'}>{p.code}</span>
        </span>
      ))}
    </>
  );
}
