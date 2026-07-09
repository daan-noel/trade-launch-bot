import { formatDurationShort, formatWithCommas } from 'utils/format';
import { Button } from './Button';

/**
 * Honest determinate progress bar for long-running batch flows (simulation,
 * grouped sweep). The backend streams real `processed / total` counts over SSE,
 * so the bar reports actual progress instead of a fake trickle. Before the first
 * frame arrives `total` is unknown (`processed`/`total` null), so it shows an
 * indeterminate pulse at a fixed width and a "Starting…" count.
 *
 * When the caller supplies `etaMs` / `elapsedMs` (derived client-side from the
 * observed throughput) the second line shows remaining-count, a ticking ETA, and
 * elapsed time. `etaMs` is null until a positive rate exists → "Estimating…".
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
  etaMs = null,
  elapsedMs = null,
  done = false,
}: {
  label: string;
  processed: number | null;
  total: number | null;
  onCancel?: () => void;
  cancelling?: boolean;
  /** Estimated ms until completion; null = not enough data yet ("Estimating…"). */
  etaMs?: number | null;
  /** Ms since we started observing this job; null hides the elapsed readout. */
  elapsedMs?: number | null;
  /** Completed phase: renders muted 100% bar with checkmark, no cancel / ETA. */
  done?: boolean;
}) {
  // Completed phases show a muted full bar with a checkmark and no interactive controls.
  if (done) {
    return (
      <div className="mt-4 opacity-50">
        <div className="mb-2 flex items-center justify-between gap-2">
          <span className="text-[11px] font-bold uppercase tracking-widest text-text-dim">
            ✓ {label}
          </span>
          <span className="font-mono text-[11px] text-text-dim">
            {total != null ? `${formatWithCommas(total)} / ${formatWithCommas(total)} · 100%` : 'Done'}
          </span>
        </div>
        <div className="h-2 overflow-hidden rounded-full bg-white/6">
          <div className="h-full rounded-full bg-text-dim transition-[width] duration-300" style={{ width: '100%' }} />
        </div>
      </div>
    );
  }

  const determinate = processed != null && total != null && total > 0;
  const percent = determinate ? Math.min(100, Math.round((processed / total) * 100)) : null;
  const remaining = determinate ? Math.max(0, total - processed) : null;

  // Second line, only when determinate: "812 left · ~2m 15s remaining · 1m 03s elapsed".
  const detailParts: string[] = [];
  if (remaining != null) detailParts.push(`${formatWithCommas(remaining)} left`);
  if (determinate)
    detailParts.push(
      etaMs != null ? `~${formatDurationShort(etaMs / 1000)} remaining` : 'estimating…',
    );
  if (elapsedMs != null) detailParts.push(`${formatDurationShort(elapsedMs / 1000)} elapsed`);

  return (
    <div className="mt-4">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-[11px] font-bold uppercase tracking-widest text-primary">
          {label}
        </span>
        <div className="flex items-center gap-3">
          <span className="font-mono text-[11px] text-text-dim">
            {determinate
              ? `${formatWithCommas(processed)} / ${formatWithCommas(total)} · ${percent}%`
              : 'Starting…'}
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
      {detailParts.length > 0 && (
        <div className="mt-1.5 font-mono text-[10px] text-text-dim">{detailParts.join(' · ')}</div>
      )}
    </div>
  );
}
