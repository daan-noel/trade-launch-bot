import { Link } from 'react-router-dom';
import { Badge } from 'components/ui/Badge';
import { useBackgroundJobsState } from '@lab/context/BackgroundJobsContext';
import { useGetGroupedSweepRunsQuery } from '@lab/store/labEndpoints';
import { GENERIC_STRATEGY_ID } from '@lab/components/sweep/GenericSweepConfigForm';

const SHORTCUTS: { to: string; label: string; blurb: string }[] = [
  { to: '/tokens', label: 'Tokens', blurb: 'Universe + metric panes' },
  { to: '/strategies/simulate', label: 'Simulate', blurb: 'Run saved rules on the lake' },
  { to: '/strategies/sweep', label: 'Grouped sweep', blurb: 'Param search → promote' },
  { to: '/analysis/trader', label: 'Trader', blurb: 'Wallet → tokens + charts' },
];

/**
 * Lab research hub — replaces the empty splash. Shortcuts + recent sweep runs
 * + active background jobs. No dashboard sprawl.
 */
export function LabHomePage() {
  const { jobs } = useBackgroundJobsState();
  const { data: runs = [] } = useGetGroupedSweepRunsQuery({ strategyId: GENERIC_STRATEGY_ID });
  const recent = runs.slice(0, 5);
  const activeJobs = jobs;

  return (
    <div className="pt-2">
      <div className="mb-4 flex flex-wrap items-baseline gap-3">
        <h1 className="text-2xl font-extrabold text-text">Research</h1>
        <span className="text-sm text-text-mid">Pick a path, or resume a recent sweep</span>
      </div>

      <div className="mb-5 grid grid-cols-2 gap-2.5 sm:grid-cols-4">
        {SHORTCUTS.map((s) => (
          <Link
            key={s.to}
            to={s.to}
            className="rounded-lg border border-white/6 bg-white/2 px-3 py-3 transition hover:border-primary/40 hover:bg-white/4"
          >
            <div className="text-sm font-bold text-text">{s.label}</div>
            <div className="mt-0.5 text-[11px] text-text-dim">{s.blurb}</div>
          </Link>
        ))}
      </div>

      {activeJobs.length > 0 && (
        <section className="mb-5">
          <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-text-dim">
            Running now
          </h2>
          <ul className="flex flex-col gap-1.5">
            {activeJobs.map((j) => (
              <li
                key={`${j.kind}:${j.id}`}
                className="flex items-center gap-2 rounded-md border border-white/6 bg-white/2 px-3 py-2 text-sm"
              >
                <Badge variant="info">{j.kind}</Badge>
                <span className="truncate text-text">{j.label}</span>
                <span className="ml-auto text-[11px] text-text-dim">
                  {j.cancelling ? 'cancelling…' : 'running'}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section>
        <div className="mb-2 flex items-center justify-between">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-text-dim">
            Recent sweeps
          </h2>
          <Link to="/strategies/sweep" className="text-[11px] text-accent hover:text-primary hover:underline">
            Open sweep →
          </Link>
        </div>
        {recent.length === 0 ? (
          <p className="rounded-md border border-dashed border-white/8 px-3 py-6 text-center text-xs text-text-dim">
            No sweep runs yet. Start one from Grouped sweep.
          </p>
        ) : (
          <ul className="flex flex-col gap-1.5">
            {recent.map((r) => {
              const d = new Date(r.created_at);
              const when = `${String(d.getMonth() + 1).padStart(2, '0')}/${String(d.getDate()).padStart(2, '0')} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
              return (
                <li key={r.id}>
                  <Link
                    to={`/strategies/sweep?run=${encodeURIComponent(r.id)}`}
                    className="flex flex-wrap items-center gap-2 rounded-md border border-white/6 bg-white/2 px-3 py-2 text-sm transition hover:border-primary/30"
                  >
                    <span className="font-mono text-[11px] text-text-dim">{when}</span>
                    <Badge variant={r.status === 'completed' ? 'success' : 'neutral'}>{r.status}</Badge>
                    <span className="text-text-mid">
                      {r.token_count.toLocaleString()} tok · {r.group_count} grp · {r.combo_count.toLocaleString()} combos
                    </span>
                    {r.label && <span className="truncate text-text-dim">· {r.label}</span>}
                  </Link>
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </div>
  );
}
