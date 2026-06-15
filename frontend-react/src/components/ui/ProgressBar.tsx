import { Button } from './Button';

/**
 * Honest determinate progress bar for long-running batch flows (simulation,
 * grouped sweep). The backend streams real `processed / total` counts over SSE,
 * so the bar reports actual progress instead of a fake trickle. Before the first
 * frame arrives `total` is unknown (`processed`/`total` null), so it shows an
 * indeterminate pulse at a fixed width and a "Starting…" count.
 *
 * Pass `onCancel` to render a Cancel button that lets the user end the process
 * before it completes; `cancelling` disables it + relabels once a cancel is
 * in flight (the request is cooperative, so completion isn't instant).
 */
export function ProgressBar({
  label,
  processed,
  total,
  onCancel,
  cancelling = false,
}: {
  label: string;
  processed: number | null;
  total: number | null;
  onCancel?: () => void;
  cancelling?: boolean;
}) {
  const determinate = processed != null && total != null && total > 0;
  const percent = determinate ? Math.min(100, Math.round((processed / total) * 100)) : null;
  return (
    <div className="mt-4">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-[11px] font-bold uppercase tracking-widest text-primary">
          {label}
        </span>
        <div className="flex items-center gap-3">
          <span className="font-mono text-[11px] text-text-dim">
            {determinate ? `${processed} / ${total} · ${percent}%` : 'Starting…'}
          </span>
          {onCancel && (
            <Button variant="danger" size="xs" onClick={onCancel} disabled={cancelling}>
              {cancelling ? 'Cancelling…' : 'Cancel'}
            </Button>
          )}
        </div>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-white/6">
        <div
          className="h-full animate-pulse rounded-full bg-primary transition-[width] duration-300"
          style={{ width: determinate ? `${percent}%` : '15%' }}
        />
      </div>
    </div>
  );
}
