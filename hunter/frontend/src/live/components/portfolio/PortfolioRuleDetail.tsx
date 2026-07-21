import { useEffect, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { StatTile } from 'components/ui/StatTile';
import { floorHref, rulesHref } from 'lib/strategy/nav';
import { formatCompact } from 'utils/format';
import {
  formatSigned,
  formatSignedPct,
  pctGradeClass,
  signedStatTone,
  signedToneClass,
} from 'lib/signedTone';
import { fetchRulePositionsPage } from 'services/api';
import { FloorMintChart } from '@live/components/floor/FloorMintChart';
import { buildEventMarkers } from 'components/strategy/inspectTarget';
import type { PortfolioRulePnl, RulePositionRecord } from 'types';

const STRATEGY_SEG = 'generic';

function rangeSinceMs(range: 'today' | '7d' | 'all'): number | null {
  if (range === 'all') return null;
  const now = Date.now();
  if (range === 'today') {
    const d = new Date();
    d.setUTCHours(0, 0, 0, 0);
    return d.getTime();
  }
  return now - 7 * 24 * 60 * 60 * 1000;
}

/**
 * Selected-rule drill-in on Portfolio — summary tiles + recent closes in range
 * + mint chart when a close row is picked.
 */
export function PortfolioRuleDetail({
  row,
  range,
  mode,
}: {
  row: PortfolioRulePnl;
  range: 'today' | '7d' | 'all';
  mode: 'real' | 'paper';
}) {
  const pnlPct =
    row.total_entry_sol > 0 ? (row.realized_pnl_sol / row.total_entry_sol) * 100 : null;
  const [closes, setCloses] = useState<RulePositionRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [pick, setPick] = useState<RulePositionRecord | null>(null);
  const sinceMs = rangeSinceMs(range);

  useEffect(() => {
    let cancelled = false;
    const ac = new AbortController();
    setLoading(true);
    setPick(null);
    void (async () => {
      try {
        const { items } = await fetchRulePositionsPage(
          STRATEGY_SEG,
          row.rule_id,
          {
            pagination: { page: 1, pageSize: 40 },
            sorting: [{ col: 'exit_time', dir: 'desc' }],
            search: '',
            filters: {},
          },
          { kind: 'all' },
          ac.signal,
        );
        if (cancelled) return;
        const closed = items.filter((p) => p.exit_time != null && p.status === 'End');
        const inRange =
          sinceMs == null
            ? closed
            : closed.filter((p) => {
                const t = Date.parse(p.exit_time!);
                return Number.isFinite(t) && t >= sinceMs;
              });
        setCloses(inRange.slice(0, 15));
      } catch {
        if (!cancelled) setCloses([]);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
      ac.abort();
    };
  }, [row.rule_id, sinceMs]);

  const markers = useMemo(
    () => (pick ? buildEventMarkers({
      mint_address: pick.mint_address,
      entryTime: pick.entry_time,
      entryPrice: pick.entry_price,
      entryTx: pick.entry_tx,
      exitTime: pick.exit_time,
      exitPrice: pick.exit_price,
      exitTx: pick.exit_tx,
      exitLabel: pick.exit_reason,
    }) : null),
    [pick],
  );

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-6">
        <StatTile
          label="Realized"
          value={`◎${formatSigned(row.realized_pnl_sol, 3)}`}
          size="sm"
          tone={signedStatTone(row.realized_pnl_sol)}
        />
        <StatTile
          label="PnL%"
          value={
            pnlPct != null ? (
              <span className={pctGradeClass(pnlPct)}>{formatSignedPct(pnlPct, 1)}</span>
            ) : (
              '—'
            )
          }
          size="sm"
        />
        <StatTile
          label="Win rate"
          value={row.closed > 0 ? `${Math.round(row.win_rate)}%` : '—'}
          size="sm"
        />
        <StatTile
          label="W/L"
          value={`${row.win}/${row.loss}`}
          size="sm"
          tone="muted"
        />
        <StatTile label="Closed" value={row.closed} size="sm" tone="muted" />
        <StatTile
          label="Entry ◎"
          value={formatCompact(row.total_entry_sol, 2)}
          size="sm"
          tone="muted"
        />
      </div>

      <div className="flex flex-wrap gap-3 text-[11px]">
        <Link
          to={rulesHref(row.rule_id)}
          className="font-semibold text-accent hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Rules Evidence
        </Link>
        <Link
          to={floorHref({ tab: 'open', mode, ruleId: row.rule_id })}
          className="font-semibold text-accent hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Floor open
        </Link>
        <Link
          to={floorHref({ tab: 'recent', mode, ruleId: row.rule_id })}
          className="font-semibold text-accent hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Floor recent
        </Link>
      </div>

      <div>
        <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-text-dim">
          Closes in range {loading ? '· loading…' : `· ${closes.length}`}
        </div>
        {closes.length === 0 && !loading ? (
          <p className="text-[11px] text-text-dim">No End closes in this window.</p>
        ) : (
          <div className="overflow-x-auto rounded-md border border-white/6">
            <table className="w-full text-left text-[11px]">
              <thead className="bg-white/[0.03] text-text-dim">
                <tr>
                  <th className="px-2 py-1 font-semibold">Token</th>
                  <th className="px-2 py-1 font-semibold">PnL</th>
                  <th className="px-2 py-1 font-semibold">PnL%</th>
                  <th className="px-2 py-1 font-semibold">Exit</th>
                </tr>
              </thead>
              <tbody>
                {closes.map((p) => {
                  const selected = pick?.id === p.id;
                  return (
                    <tr
                      key={p.id}
                      className={`cursor-pointer border-t border-white/5 hover:bg-white/[0.04] ${
                        selected ? 'bg-primary/10' : ''
                      }`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setPick(selected ? null : p);
                      }}
                    >
                      <td className="px-2 py-1">
                        <AddressDisplay address={p.mint_address} kind="token" />
                      </td>
                      <td className={`px-2 py-1 tabular-nums ${signedToneClass(p.pnl_sol)}`}>
                        {p.pnl_sol != null ? `${formatSigned(p.pnl_sol, 3)}◎` : '—'}
                      </td>
                      <td
                        className={`px-2 py-1 tabular-nums ${
                          p.pnl_percent != null ? pctGradeClass(p.pnl_percent) : 'text-text-dim'
                        }`}
                      >
                        {p.pnl_percent != null ? formatSignedPct(p.pnl_percent, 1) : '—'}
                      </td>
                      <td className="px-2 py-1 text-text-dim">{p.exit_reason ?? '—'}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {pick && (
        <FloorMintChart
          mint={pick.mint_address}
          markers={markers}
          tableId="portfolio-rule-detail"
        />
      )}
    </div>
  );
}
