import { dashPercent, dashF } from 'components/strategy/cellFormat';
import type { PositionsSummary } from 'types';

/** Compact funnel summary of a finished simulation run (dry-run + SimulatePage).
 *  `PositionsSummary` is the population-wide aggregate the sim result-summary
 *  endpoint returns (SOL fields are human SOL; win_rate/avg_pnl_pct are percents).
 *  NOTE: an exit-reason breakdown (TP / SL / metrics / dead) is not in this
 *  summary — surfacing it would need a richer aggregate (deferred). */
export function SimSummary({ summary }: { summary: PositionsSummary }) {
  const cells: Array<[string, string]> = [
    ['entered', String(summary.tokens)],
    ['open', String(summary.open)],
    ['win', String(summary.win)],
    ['loss', String(summary.loss)],
    ['win rate', dashPercent(summary.win_rate)],
    ['avg pnl', dashPercent(summary.avg_pnl_pct)],
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
        <b className={summary.total_pnl_sol >= 0 ? 'text-green tabular-nums' : 'text-red tabular-nums'}>
          {dashF(summary.total_pnl_sol, 3)}◎
        </b>
      </span>
    </div>
  );
}
