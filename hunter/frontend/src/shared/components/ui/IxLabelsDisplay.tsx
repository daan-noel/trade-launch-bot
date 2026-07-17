import { useMemo, useState, type CSSProperties, type MouseEvent } from 'react';
import { formatIxLabelsText } from 'lib/ixLabels';
import { cn } from 'lib/cn';

export interface IxLabelsDisplayProps {
  labels: string[];
  /** Click copies the pretty-printed JSON (default false). */
  copyJson?: boolean;
  /** Cap height for dense table cells; overflow scrolls. CSS length, e.g. `4.5rem`. */
  maxHeight?: string;
  /** Shown when `labels` is empty. Omit to render nothing. */
  empty?: string;
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
