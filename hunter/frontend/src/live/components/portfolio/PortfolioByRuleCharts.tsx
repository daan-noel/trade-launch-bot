import { formatSigned, signedToneClass } from 'lib/signedTone';
import type { PortfolioRulePnl } from 'types';

/** Horizontal PnL / PnL% bars for the Portfolio page. */
export function PortfolioByRuleCharts({ rows }: { rows: PortfolioRulePnl[] }) {
  const sortedPnl = [...rows].sort((a, b) => b.realized_pnl_sol - a.realized_pnl_sol);
  const withPct = sortedPnl.map((r) => ({
    ...r,
    pnlPct:
      r.total_entry_sol > 0 ? (r.realized_pnl_sol / r.total_entry_sol) * 100 : null,
  }));
  const maxAbsPnl = Math.max(...sortedPnl.map((r) => Math.abs(r.realized_pnl_sol)), 1e-9);
  const maxAbsPct = Math.max(
    ...withPct.map((r) => (r.pnlPct != null ? Math.abs(r.pnlPct) : 0)),
    1e-9,
  );

  if (rows.length === 0) return null;

  return (
    <div className="grid gap-3 md:grid-cols-2">
      <BarCard
        title="Realized PnL by rule"
        rows={sortedPnl.map((r) => ({
          key: r.rule_id,
          label: r.rule_name ?? r.rule_id.slice(0, 8),
          value: r.realized_pnl_sol,
          display: `${formatSigned(r.realized_pnl_sol, 3)}◎`,
          widthPct: Math.max(4, Math.round((Math.abs(r.realized_pnl_sol) / maxAbsPnl) * 100)),
        }))}
      />
      <BarCard
        title="PnL% by rule"
        rows={withPct.map((r) => ({
          key: r.rule_id,
          label: r.rule_name ?? r.rule_id.slice(0, 8),
          value: r.pnlPct ?? 0,
          display: r.pnlPct != null ? `${formatSigned(r.pnlPct, 1)}%` : '—',
          widthPct:
            r.pnlPct != null
              ? Math.max(4, Math.round((Math.abs(r.pnlPct) / maxAbsPct) * 100))
              : 0,
        }))}
      />
    </div>
  );
}

function BarCard({
  title,
  rows,
}: {
  title: string;
  rows: Array<{
    key: string;
    label: string;
    value: number;
    display: string;
    widthPct: number;
  }>;
}) {
  return (
    <div className="rounded-lg border border-white/6 bg-white/[0.02] px-3 py-2">
      <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-text-dim">
        {title}
      </div>
      <div className="flex flex-col gap-1">
        {rows.slice(0, 10).map((r) => (
          <div key={r.key} className="flex items-center gap-2 text-[11px]">
            <span className="w-24 shrink-0 truncate text-text-mid" title={r.label}>
              {r.label}
            </span>
            <div className="relative h-2 min-w-0 flex-1 rounded-sm bg-white/5">
              <div
                className={`absolute inset-y-0 left-0 rounded-sm ${
                  r.value >= 0 ? 'bg-green/70' : 'bg-red/70'
                }`}
                style={{ width: `${r.widthPct}%` }}
              />
            </div>
            <span
              className={`w-16 shrink-0 text-right tabular-nums ${signedToneClass(r.value)}`}
            >
              {r.display}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
