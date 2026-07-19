import { goodBad, pctText, solText } from 'lib/strategy/runSummary';
import type { SimulatedSummary } from 'types';

/** Compact funnel summary of a finished simulation run (dry-run panel).
 *  SimulatePage renders the same fields as separate DataTable columns instead.
 *
 *  Wire shape is the shared two-band `RunSummary` — `realized` is closed-only,
 *  `mtm` values the open positions too. Both PnL figures are shown because the
 *  realized one alone flatters a run that never closed its losers; the same
 *  reasoning (and the same formatters) as the full summary panel. */
export function SimSummary({ summary }: { summary: SimulatedSummary }) {
  const { realized, mtm } = summary;
  const cells: Array<[string, string]> = [
    ['entered', String(realized.n_fired)],
    ['closed', String(realized.n_closed)],
    ['open', String(realized.n_open)],
    ['win rate', realized.n_closed ? `${(realized.win_rate * 100).toFixed(0)}%` : '—'],
    ['mean', realized.n_closed ? pctText(realized.mean_pnl_pct) : '—'],
  ];
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[12px]">
      {cells.map(([k, v]) => (
        <span key={k} className="text-text-dim">
          {k} <b className="text-text tabular-nums">{v}</b>
        </span>
      ))}
      <span className="text-text-dim">
        PnL{' '}
        <b className={`tabular-nums ${goodBad(realized.total_pnl_sol)}`}>
          {solText(realized.total_pnl_sol)}
        </b>
      </span>
      {realized.n_open > 0 && (
        <span className="text-text-dim">
          incl. open{' '}
          <b className={`tabular-nums ${goodBad(mtm.total_pnl_sol)}`}>
            {solText(mtm.total_pnl_sol)}
          </b>
        </span>
      )}
    </div>
  );
}
