import { useEffect, useState } from 'react';
import { ProgressBar } from 'components/ui/ProgressBar';
import {
  useBackgroundJobActions,
  useBackgroundJobsState,
  type BackgroundJob,
  type PhaseProgress,
} from 'context/BackgroundJobsContext';

/**
 * App-wide floating indicator for in-flight background jobs (grouped sweep, rule
 * simulations). Driven entirely by `BackgroundJobsContext`, so it stays visible
 * and live as the user navigates between pages — the job runs on the backend
 * regardless, and this is the one place the UI tracks it. Renders nothing when
 * idle. Each row shows `processed / total`, a ticking ETA + elapsed time, and a
 * Cancel button.
 *
 * The ETA/elapsed countdown ticks once a second from a local timer that runs
 * ONLY while a job is present (the effect re-subscribes when the job set
 * changes). It's confined to this tiny isolated component, so it never drags the
 * SOL/USD or live-trade renders.
 */

/** Average-throughput ETA (ms) since we first observed the job, or null when no
 *  positive rate is measurable yet (processed hasn't advanced past baseline). */
function estimateEtaMs(job: BackgroundJob, now: number): number | null {
  if (job.processed == null || job.total == null || job.total <= 0) return null;
  const done = job.processed - job.firstSeenProcessed;
  const elapsed = now - job.firstSeenAt;
  if (done <= 0 || elapsed <= 0) return null;
  const remaining = job.total - job.processed;
  if (remaining <= 0) return 0;
  return (remaining / done) * elapsed;
}

export function BackgroundJobsIndicator() {
  const { jobs } = useBackgroundJobsState();
  const { cancel } = useBackgroundJobActions();

  // Drives the 1s countdown between sparse SSE frames; only ticks while jobs run.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (jobs.length === 0) return;
    setNow(Date.now());
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [jobs.length]);

  if (jobs.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
      {jobs.map((job) => (
        <div
          key={`${job.kind}:${job.id}`}
          className="rounded-md border border-white/10 bg-surface/95 px-3 pb-3 pt-1 shadow-lg backdrop-blur"
        >
          {job.phases.size > 0
            ? Array.from(job.phases.entries()).map(([phaseKey, ph]: [string, PhaseProgress]) => (
                <ProgressBar
                  key={phaseKey}
                  label={ph.label}
                  processed={ph.done ? ph.total : ph.processed}
                  total={ph.total}
                  cancelling={phaseKey === job.activePhase ? job.cancelling : false}
                  onCancel={phaseKey === job.activePhase ? () => cancel(job) : undefined}
                  etaMs={phaseKey === job.activePhase ? estimateEtaMs(job, now) : null}
                  elapsedMs={phaseKey === job.activePhase ? now - job.firstSeenAt : null}
                  done={ph.done}
                />
              ))
            : (
              <ProgressBar
                label={job.label}
                processed={job.processed}
                total={job.total}
                cancelling={job.cancelling}
                etaMs={estimateEtaMs(job, now)}
                elapsedMs={now - job.firstSeenAt}
                onCancel={() => cancel(job)}
              />
            )}
        </div>
      ))}
    </div>
  );
}
