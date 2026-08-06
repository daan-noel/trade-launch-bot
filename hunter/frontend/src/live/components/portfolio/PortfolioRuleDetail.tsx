import { useCallback, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { StatTile } from 'components/ui/StatTile';
import { consoleHistoryHref, rulesHref, type HistoryRange } from 'lib/strategy/nav';
import { formatCompact } from 'utils/format';
import {
  formatSigned,
  formatSignedPct,
  pctGradeClass,
  signedStatTone,
  signedToneClass,
} from 'lib/signedTone';
import { exitReasonBadge } from 'components/strategy/strategyColumns';
import { fetchPortfolioPositionsPage } from 'services/api';
import { useServerTable } from 'hooks/useServerTable';
import { FloorMintChart } from '@live/components/floor/FloorMintChart';
import { buildEventMarkers, inspectFromPosition } from 'components/strategy/inspectTarget';
import type { PortfolioRulePnl, RulePositionRecord } from 'types';

const PAGE_SIZE = 15;

/** The page's range keyword → the window's UTC start (`null` = all-time). */
function rangeSinceIso(range: 'today' | '7d' | '30d' | 'all', nowMs: number): string | null {
  if (range === 'all') return null;
  if (range === 'today') {
    const d = new Date(nowMs);
    return new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate())).toISOString();
  }
  const days = range === '7d' ? 7 : 30;
  return new Date(nowMs - days * 86_400_000).toISOString();
}

/**
 * Selected-rule drill-in on Portfolio — summary tiles + the rule's closes in the
 * page's window + a mint chart when a close row is picked.
 *
 * The closes come from the SAME cross-rule endpoint the Console History table
 * pages (`POST /api/portfolio/positions/query`), with rule / mode / status and
 * the time window applied **server-side**. The previous version fetched 40 rows
 * and then filtered them down client-side, so a rule with more than 40 recent
 * positions could show zero in-range closes while having plenty.
 */
export function PortfolioRuleDetail({
  row,
  range,
  mode,
}: {
  row: PortfolioRulePnl;
  range: 'today' | '7d' | '30d' | 'all';
  mode: 'real' | 'paper';
}) {
  const pnlPct =
    row.total_entry_sol > 0 ? (row.realized_pnl_sol / row.total_entry_sol) * 100 : null;
  const [pick, setPick] = useState<RulePositionRecord | null>(null);
  // Frozen per mount so the window bound can't slide between fetches.
  const [nowMs] = useState(() => Date.now());
  const fromIso = rangeSinceIso(range, nowMs);

  const body = useMemo(
    () => ({
      pagination: { page: 1, pageSize: PAGE_SIZE },
      sorting: [{ col: 'exit_time', dir: 'desc' as const }],
      search: '',
      filters: {
        rule_id: { op: 'eq' as const, val: row.rule_id },
        mode: { op: 'eq' as const, val: mode },
        status: { op: 'eq' as const, val: 'End' },
      },
      ...(fromIso ? { range: { from: fromIso } } : {}),
    }),
    [row.rule_id, mode, fromIso],
  );

  const fetchPage = useCallback(
    (b: unknown, signal: AbortSignal) => fetchPortfolioPositionsPage(b as never, signal),
    [],
  );

  const { items: closes, total, loading } = useServerTable<RulePositionRecord>(
    true,
    body,
    fetchPage,
  );

  const markers = useMemo(
    () => (pick ? buildEventMarkers(inspectFromPosition(pick)) : null),
    [pick],
  );

  const historyRange: HistoryRange = range;

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
        <StatTile label="W/L" value={`${row.win}/${row.loss}`} size="sm" tone="muted" />
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
          to={consoleHistoryHref({ ruleId: row.rule_id, range: historyRange, mode })}
          className="font-semibold text-accent hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Full history →
        </Link>
      </div>

      <div>
        <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-text-dim">
          Closes in range{' '}
          {loading ? '· loading…' : `· showing ${closes.length} of ${total}`}
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
                      <td className="px-2 py-1">
                        {exitReasonBadge(p.exit_reason, p.pnl_sol, p.last_entry_error)}
                      </td>
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
