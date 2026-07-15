import { useBackgroundJobsState } from '@lab/context/BackgroundJobsContext';

/**
 * Compact inline progress bar for a rule row whose backtest is in flight — the
 * per-row twin of the app-wide {@link BackgroundJobsIndicator}. Reads the shared
 * background-jobs registry (fed by the `simulation_progress` SSE, keyed by
 * `rule_id`), so it lights up for BOTH a single 🧪 Simulate and every rule queued
 * by "Simulate All" without the page having to thread progress through props.
 *
 * Isolated as its own leaf so only these tiny bars re-render on a progress tick —
 * the heavy rules page consumes the *actions* context only and stays put. Renders
 * nothing (no layout shift) when the rule isn't currently simulating.
 *
 * Injected into the shared `ruleColumns` `AnalysisControls` cell from the page so
 * that shared column never imports the `@lab` context directly (`shared ⊬ @lab`).
 */
export function RuleSimProgressBar({ ruleId }: { ruleId: string }) {
  const { jobs } = useBackgroundJobsState();
  const job = jobs.find((j) => j.kind === 'simulation' && j.id === ruleId);
  if (!job) return null;

  const determinate = job.processed != null && job.total != null && job.total > 0;
  const percent = determinate
    ? Math.min(100, Math.round((job.processed! / job.total!) * 100))
    : null;

  return (
    <div
      className="mt-1 w-full"
      title={
        determinate
          ? `Simulating — ${job.processed} / ${job.total} tokens`
          : 'Simulating — starting…'
      }
    >
      <div className="h-1 overflow-hidden rounded-full bg-white/6">
        <div
          className="h-full animate-pulse rounded-full bg-primary transition-[width] duration-300"
          style={{ width: determinate ? `${percent}%` : '20%' }}
        />
      </div>
      <div className="mt-0.5 text-center font-mono text-[9px] leading-none text-text-dim">
        {determinate ? `${percent}%` : '…'}
      </div>
    </div>
  );
}
