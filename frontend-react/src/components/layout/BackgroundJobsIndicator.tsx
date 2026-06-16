import { ProgressBar } from 'components/ui/ProgressBar';
import { useBackgroundJobs } from 'context/BackgroundJobsContext';

/**
 * App-wide floating indicator for in-flight background jobs (grouped sweep, rule
 * simulations). Driven entirely by `BackgroundJobsContext`, so it stays visible
 * and live as the user navigates between pages — the job runs on the backend
 * regardless, and this is the one place the UI tracks it. Renders nothing when
 * idle. Each row shows real `processed / total` progress and a Cancel button.
 */
export function BackgroundJobsIndicator() {
  const { jobs, cancel } = useBackgroundJobs();
  if (jobs.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
      {jobs.map((job) => (
        <div
          key={`${job.kind}:${job.id}`}
          className="rounded-md border border-white/10 bg-surface/95 px-3 pb-3 pt-1 shadow-lg backdrop-blur"
        >
          <ProgressBar
            label={job.label}
            processed={job.processed}
            total={job.total}
            cancelling={job.cancelling}
            onCancel={() => cancel(job)}
          />
        </div>
      ))}
    </div>
  );
}
