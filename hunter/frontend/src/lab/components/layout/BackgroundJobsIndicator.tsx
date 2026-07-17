import { useEffect, useState } from 'react';
import { ProgressBar } from 'components/ui/ProgressBar';
import { cn } from 'lib/cn';
import {
  useBackgroundJobActions,
  useBackgroundJobsState,
  type BackgroundJob,
  type PhaseProgress,
} from '@lab/context/BackgroundJobsContext';

/**
 * App-wide floating indicator for in-flight background jobs (grouped sweep, rule
 * simulations). Driven entirely by `BackgroundJobsContext`, so it stays visible
 * and live as the user navigates between pages — the job runs on the backend
 * regardless, and this is the one place the UI tracks it. Renders nothing when
 * idle. Each row shows `processed / total`, a ticking ETA + elapsed time, and a
 * Cancel button.
 *
 * Simulation jobs the mounted strategy page already shows per-row (its
 * `coveredSimIds`) are suppressed here — the inline row bars cover them while
 * you're on that page. The remaining simulation jobs (fired from a rule you've
 * since navigated away from, or a different strategy) are collapsed into ONE
 * card: a summary line over all of them plus a compact line per rule. Sweeps and
 * swings keep their own individual cards (their phase/ETA detail can't fold into
 * a shared summary).
 *
 * The ETA/elapsed countdown ticks once a second from a local timer that runs
 * ONLY while a job is present (the effect re-subscribes when the job set
 * changes). It's confined to this tiny isolated component, so it never drags the
 * SOL/USD or live-trade renders.
 */

/** Just the fields the ETA needs — lets the same helper price a real job or the
 *  synthetic aggregate we build for the grouped simulation summary. */
type EtaInput = Pick<BackgroundJob, 'processed' | 'total' | 'firstSeenProcessed' | 'firstSeenAt'>;

/** Average-throughput ETA (ms) since we first observed the job, or null when no
 *  positive rate is measurable yet (processed hasn't advanced past baseline). */
function estimateEtaMs(job: EtaInput, now: number): number | null {
  if (job.processed == null || job.total == null || job.total <= 0) return null;
  const done = job.processed - job.firstSeenProcessed;
  const elapsed = now - job.firstSeenAt;
  if (done <= 0 || elapsed <= 0) return null;
  const remaining = job.total - job.processed;
  if (remaining <= 0) return 0;
  return (remaining / done) * elapsed;
}

/** One job rendered as its own card — the sweep/swing case (with phases) and the
 *  lone-simulation case both flow through here. */
function JobCard({ job, now, onCancel }: { job: BackgroundJob; now: number; onCancel: () => void }) {
  return (
    <div className="rounded-md border border-white/10 bg-surface/95 px-3 pb-3 pt-1 shadow-lg backdrop-blur">
      {job.phases.size > 0 ? (
        Array.from(job.phases.entries()).map(([phaseKey, ph]: [string, PhaseProgress]) => (
          <ProgressBar
            key={phaseKey}
            label={ph.label}
            processed={ph.done ? ph.total : ph.processed}
            total={ph.total}
            cancelling={phaseKey === job.activePhase ? job.cancelling : false}
            onCancel={phaseKey === job.activePhase ? onCancel : undefined}
            etaMs={phaseKey === job.activePhase ? estimateEtaMs(job, now) : null}
            elapsedMs={phaseKey === job.activePhase ? now - job.firstSeenAt : null}
            done={ph.done}
          />
        ))
      ) : (
        <ProgressBar
          label={job.label}
          processed={job.processed}
          total={job.total}
          cancelling={job.cancelling}
          etaMs={estimateEtaMs(job, now)}
          elapsedMs={now - job.firstSeenAt}
          onCancel={onCancel}
        />
      )}
      {job.groupCount != null && (
        // Persistence is a concurrent drain, not a phase — groups commit while
        // later groups still fold, and each committed group is browsable NOW.
        <div className="mt-1.5 flex items-center justify-between border-t border-white/8 pt-1.5 text-[10px] text-text-dim">
          <span>Groups saved (browsable now)</span>
          <span className="font-mono">
            {job.groupsDone ?? 0}/{job.groupCount}
          </span>
        </div>
      )}
    </div>
  );
}

/** Compact per-rule line inside the grouped simulation card: a thin bar + percent
 *  + a small ✕ that cancels just this rule's run. */
function SimLine({ job, onCancel }: { job: BackgroundJob; onCancel: () => void }) {
  const determinate = job.processed != null && job.total != null && job.total > 0;
  const percent = determinate ? Math.min(100, Math.round((job.processed! / job.total!) * 100)) : null;
  return (
    <div className="flex items-center gap-2">
      <div className="min-w-0 flex-1">
        <div className="mb-1 flex items-center justify-between gap-2">
          <span className="truncate text-[10px] text-text-dim" title={job.label}>
            {job.label}
          </span>
          <span className="shrink-0 font-mono text-[10px] text-text-dim">
            {determinate ? `${percent}%` : '…'}
          </span>
        </div>
        <div className="h-1 overflow-hidden rounded-full bg-white/6">
          <div
            className={cn(
              'h-full rounded-full bg-primary transition-[width] duration-300',
              !determinate && 'animate-pulse',
            )}
            style={{ width: determinate ? `${percent}%` : '20%' }}
          />
        </div>
      </div>
      <button
        type="button"
        onClick={onCancel}
        disabled={job.cancelling}
        title={job.cancelling ? 'Cancelling…' : 'Cancel this simulation'}
        className="shrink-0 text-text-dim transition hover:text-red disabled:opacity-40"
      >
        ✕
      </button>
    </div>
  );
}

/** Two or more off-page simulations, collapsed into one card: a token-weighted
 *  summary bar (with a Cancel-all) over a compact line per rule. */
function SimGroupCard({
  simJobs,
  now,
  onCancel,
}: {
  simJobs: BackgroundJob[];
  now: number;
  onCancel: (job: BackgroundJob) => void;
}) {
  // Aggregate across the rules that already know their token total; the summary
  // bar fills by Σprocessed / Σtotal so it advances smoothly, and `done` counts
  // rules that have hit 100% for the "3/5 rules" headline.
  let processed = 0;
  let total = 0;
  let done = 0;
  let firstSeenAt = now;
  let firstSeenProcessed = 0;
  for (const j of simJobs) {
    firstSeenAt = Math.min(firstSeenAt, j.firstSeenAt);
    firstSeenProcessed += j.firstSeenProcessed;
    if (j.total != null && j.total > 0) {
      total += j.total;
      processed += Math.min(j.processed ?? 0, j.total);
      if (j.processed != null && j.processed >= j.total) done += 1;
    }
  }
  const agg: EtaInput = {
    processed: total > 0 ? processed : null,
    total: total > 0 ? total : null,
    firstSeenProcessed,
    firstSeenAt,
  };
  const allCancelling = simJobs.every((j) => j.cancelling);

  return (
    <div className="rounded-md border border-white/10 bg-surface/95 px-3 pb-3 pt-1 shadow-lg backdrop-blur">
      <ProgressBar
        label={`Simulating · ${done}/${simJobs.length} rules done`}
        processed={agg.processed}
        total={agg.total}
        cancelling={allCancelling}
        etaMs={estimateEtaMs(agg, now)}
        elapsedMs={now - firstSeenAt}
        onCancel={() => simJobs.forEach(onCancel)}
      />
      <div className="mt-3 flex flex-col gap-2 border-t border-white/8 pt-2.5">
        {simJobs.map((job) => (
          <SimLine key={job.id} job={job} onCancel={() => onCancel(job)} />
        ))}
      </div>
    </div>
  );
}

export function BackgroundJobsIndicator() {
  const { jobs, coveredSimIds } = useBackgroundJobsState();
  const { cancel } = useBackgroundJobActions();

  // Simulations the active strategy page shows per-row are covered — hide them
  // here. A lone remaining simulation renders as its own card (a summary over one
  // identical line would just be noise); two or more collapse into a group.
  const visibleSims = jobs.filter((j) => j.kind === 'simulation' && !coveredSimIds.has(j.id));
  const grouped = visibleSims.length >= 2 ? visibleSims : [];
  const soloCards = jobs.filter(
    (j) => j.kind !== 'simulation' || (visibleSims.length === 1 && !coveredSimIds.has(j.id)),
  );

  const visibleCount = soloCards.length + grouped.length;

  // Drives the 1s countdown between sparse SSE frames; only ticks while something
  // is actually on screen.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (visibleCount === 0) return;
    setNow(Date.now());
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [visibleCount]);

  if (visibleCount === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
      {soloCards.map((job) => (
        <JobCard key={`${job.kind}:${job.id}`} job={job} now={now} onCancel={() => cancel(job)} />
      ))}
      {grouped.length > 0 && <SimGroupCard simJobs={grouped} now={now} onCancel={cancel} />}
    </div>
  );
}
