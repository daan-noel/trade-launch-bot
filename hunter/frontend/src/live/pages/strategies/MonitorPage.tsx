import { useEffect, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { StatTile } from 'components/ui/StatTile';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { connectArmedChanged, connectStrategyPositionUpdate } from 'services/sse';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { useGetArmedQuery } from '@live/store/liveEndpoints';

type ArmedRow = { key: string; ruleId: string; mint: string; armedAt: number };
type HoldingRow = {
  positionId: string;
  ruleId: string;
  mint: string;
  status: string;
  entryPrice: number | null;
};

const HOLDING_STATUSES = new Set(['BuySubmitted', 'Holding', 'ExitPending']);
const armedKey = (ruleId: string, mint: string) => `${ruleId}|${mint}`;

/**
 * Armed monitor (live app): the generic engine's armed (token, rule) pairs and
 * open holdings in real time. Armed state is push-only via `strategy_armed_changed`
 * SSE; positions ride `strategy_position_update`. Session disarm reasons are the
 * explicit "why it left armed" signal available today.
 */
export function MonitorPage() {
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const { data: snapshot, refetch } = useGetArmedQuery();

  const [armed, setArmed] = useState<Map<string, ArmedRow>>(new Map());
  const [holding, setHolding] = useState<Map<string, HoldingRow>>(new Map());
  const [disarmedToday, setDisarmedToday] = useState<Record<string, number>>({});
  const [enteredToday, setEnteredToday] = useState(0);
  const [now, setNow] = useState(() => performance.now());

  const rulesById = useMemo(() => new Map(rules.map((r) => [r.id, r.rule_name])), [rules]);

  useEffect(() => {
    if (!snapshot) return;
    setArmed((prev) => {
      const next = new Map<string, ArmedRow>();
      for (const e of snapshot) {
        const key = armedKey(e.rule_id, e.mint_address);
        next.set(key, prev.get(key) ?? { key, ruleId: e.rule_id, mint: e.mint_address, armedAt: Date.now() });
      }
      return next;
    });
  }, [snapshot]);

  useEffect(() => {
    const h = connectArmedChanged(
      (d) => {
        const key = armedKey(d.rule_id, d.mint_address);
        if (d.state === 'armed') {
          setArmed((prev) => {
            if (prev.has(key)) return prev;
            const next = new Map(prev);
            next.set(key, { key, ruleId: d.rule_id, mint: d.mint_address, armedAt: Date.now() });
            return next;
          });
        } else {
          setArmed((prev) => {
            if (!prev.has(key)) return prev;
            const next = new Map(prev);
            next.delete(key);
            return next;
          });
          const reason = d.reason ?? 'other';
          setDisarmedToday((c) => ({ ...c, [reason]: (c[reason] ?? 0) + 1 }));
        }
      },
      () => void refetch(),
    );
    return () => h.close();
  }, [refetch]);

  useEffect(() => {
    const h = connectStrategyPositionUpdate((d) => {
      setHolding((prev) => {
        const next = new Map(prev);
        if (HOLDING_STATUSES.has(d.status)) {
          if (d.status === 'Holding' && !prev.has(d.position_id)) setEnteredToday((n) => n + 1);
          next.set(d.position_id, {
            positionId: d.position_id,
            ruleId: d.rule_id,
            mint: d.mint_address,
            status: d.status,
            entryPrice: d.entry_price ?? null,
          });
        } else {
          next.delete(d.position_id);
        }
        return next;
      });
    });
    return () => h.close();
  }, []);

  useEffect(() => {
    const id = setInterval(() => setNow(performance.now()), 1000);
    return () => clearInterval(id);
  }, []);

  const armedRows = useMemo(() => [...armed.values()], [armed]);
  const holdingRows = useMemo(() => [...holding.values()], [holding]);
  const ruleName = (id: string) => rulesById.get(id) ?? id.slice(0, 8);

  const armedColumns: ColumnDef<ArmedRow>[] = [
    {
      key: 'mint',
      label: 'Token',
      render: (r) => <AddressDisplay address={r.mint} kind="token" />,
      searchValue: (r) => r.mint,
    },
    {
      key: 'rule',
      label: 'Rule',
      render: (r) => <span className="text-text">{ruleName(r.ruleId)}</span>,
      searchValue: (r) => ruleName(r.ruleId),
    },
    {
      key: 'status',
      label: 'Status',
      render: () => <Badge variant="info">Waiting for entry</Badge>,
      searchValue: () => 'waiting',
    },
    {
      key: 'age',
      label: 'Age',
      render: (r) => <span className="tabular-nums text-text-dim">{fmtAge(Date.now() - r.armedAt)}</span>,
      searchValue: () => '',
      sortValue: (r) => r.armedAt,
      sortable: true,
    },
    {
      key: 'trade',
      label: '',
      width: '64px',
      render: (r) => (
        <Link
          to={`/trade?mint=${encodeURIComponent(r.mint)}`}
          className="text-[11px] font-semibold text-accent hover:text-primary hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Trade
        </Link>
      ),
      searchValue: () => '',
    },
  ];

  const holdingColumns: ColumnDef<HoldingRow>[] = [
    {
      key: 'mint',
      label: 'Token',
      render: (r) => <AddressDisplay address={r.mint} kind="token" />,
      searchValue: (r) => r.mint,
    },
    {
      key: 'rule',
      label: 'Rule',
      render: (r) => <span className="text-text">{ruleName(r.ruleId)}</span>,
      searchValue: (r) => ruleName(r.ruleId),
    },
    {
      key: 'status',
      label: 'Status',
      render: (r) => (
        <Badge variant={r.status === 'Holding' ? 'success' : 'info'}>{r.status}</Badge>
      ),
      searchValue: (r) => r.status,
    },
    {
      key: 'entry',
      label: 'Entry price',
      render: (r) => <span className="tabular-nums">{r.entryPrice ?? '—'}</span>,
      searchValue: (r) => String(r.entryPrice ?? ''),
    },
    {
      key: 'trade',
      label: '',
      width: '64px',
      render: (r) => (
        <Link
          to={`/trade?mint=${encodeURIComponent(r.mint)}`}
          className="text-[11px] font-semibold text-accent hover:text-primary hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Trade
        </Link>
      ),
      searchValue: () => '',
    },
  ];

  const disarmedTotal = Object.values(disarmedToday).reduce((a, b) => a + b, 0);
  const disarmSub =
    Object.entries(disarmedToday)
      .map(([k, v]) => `${k} ${v}`)
      .join(' · ') || undefined;
  void now;

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-baseline gap-3">
        <h1 className="text-lg font-extrabold text-text">Armed</h1>
        <span className="text-sm text-text-mid">Rules waiting on entry · open holdings</span>
      </div>

      <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-4">
        <StatTile label="Armed" value={armed.size} tone="primary" />
        <StatTile label="Holding" value={holding.size} tone="green" />
        <StatTile label="Entered (session)" value={enteredToday} />
        <StatTile label="Disarmed (session)" value={disarmedTotal} sub={disarmSub} tone="muted" />
      </div>

      {disarmedTotal > 0 && (
        <p className="text-xs text-text-dim">
          Why left armed this session:{' '}
          <span className="text-text-mid">{disarmSub}</span>
        </p>
      )}

      <section className="flex flex-col gap-2">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-text-dim">
          Armed ({armed.size})
        </h2>
        <DataTable
          columns={armedColumns}
          rows={armedRows}
          rowKey={(r) => r.key}
          searchable
          defaultSort={{ col: 'age', dir: 'desc' }}
          tableId="monitor-armed"
          emptyMessage="Nothing armed right now."
        />
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-text-dim">
          Holding ({holding.size})
        </h2>
        <DataTable
          columns={holdingColumns}
          rows={holdingRows}
          rowKey={(r) => r.positionId}
          tableId="monitor-holding"
          emptyMessage="No open positions."
        />
      </section>
    </div>
  );
}

function fmtAge(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}
