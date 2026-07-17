import { dashPercent, dashF } from 'components/strategy/cellFormat';
import type { SimulatedSummary } from 'types';

/** Compact funnel summary of a finished simulation run (dry-run panel).
 *  SimulatePage renders the same fields as separate DataTable columns instead.
 *  Wire shape is the lab `sim_result_summary` rollup (`SimulatedSummary`):
 *  `win_rate` is a 0..1 fraction; `avg_pnl_percent` / `total_pnl_sol` are already
 *  display units. */
export function SimSummary({ summary }: { summary: SimulatedSummary }) {
  const winPct = summary.win_rate != null ? summary.win_rate * 100 : null;
  const cells: Array<[string, string]> = [
    ['entered', String(summary.total_tokens ?? '—')],
    ['closed', String(summary.closed_tokens ?? '—')],
    ['win rate', dashPercent(winPct)],
    ['avg pnl', dashPercent(summary.avg_pnl_percent)],
  ];
  const pnl = summary.total_pnl_sol;
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[12px]">
      {cells.map(([k, v]) => (
        <span key={k} className="text-text-dim">
          {k} <b className="text-text tabular-nums">{v}</b>
        </span>
      ))}
      <span className="text-text-dim">
        PnL{' '}
        <b
          className={
            pnl == null || !Number.isFinite(pnl)
              ? 'text-text tabular-nums'
              : pnl >= 0
                ? 'text-green tabular-nums'
                : 'text-red tabular-nums'
          }
        >
          {dashF(pnl, 3)}◎
        </b>
      </span>
    </div>
  );
}
