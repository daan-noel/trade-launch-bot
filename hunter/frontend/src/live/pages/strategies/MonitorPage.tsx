import { useEffect, useMemo, useState } from 'react';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
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
 * Live monitor (live app): the generic engine's armed (token, rule) pairs and
 * open positions in real time, plus a session stat strip. Armed state is push-only
 * — it is fed by the `strategy_armed_changed` SSE with the `/api/strategies/armed`
 * snapshot as the initial + reconnect refetch; positions ride
 * `strategy_position_update`.
 *
 * NOTE: the rich "blocking condition / value→need / ETA / disarm countdown"
 * columns (FE plan §3.2) are deferred — they need the engine to enrich `ArmedDelta`
 * with per-condition evaluation detail (it currently carries only mint/rule/state).
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

  // Seed / reseed the armed map from the snapshot, preserving the client-observed
  // arm time for pairs we already know (the snapshot carries no timestamp).
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

  // Live armed deltas.
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

  // Live position deltas → holding table + entered counter.
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
          next.delete(d.position_id); // End / ExitFailed / ExitUnconfirmed
        }
        return next;
      });
    });
    return () => h.close();
  }, []);

  // One page-level 1s tick drives the age column (bounded armed set — cheap).
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
      key: 'age',
      label: 'Age',
      render: (r) => <span className="tabular-nums text-text-dim">{fmtAge(Date.now() - r.armedAt)}</span>,
      searchValue: () => '',
      sortValue: (r) => r.armedAt,
      sortable: true,
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
  ];

  const disarmedTotal = Object.values(disarmedToday).reduce((a, b) => a + b, 0);
  void now; // referenced so the 1s tick re-renders the age column

  return (
    <div className="flex flex-col gap-4 p-4">
      <h1 className="text-lg font-semibold text-text">Live monitor</h1>
      <div className="flex flex-wrap gap-3">
        <Stat label="armed" value={armed.size} tone="info" />
        <Stat label="holding" value={holding.size} tone="success" />
        <Stat label="entered (session)" value={enteredToday} />
        <Stat
          label="disarmed (session)"
          value={disarmedTotal}
          sub={Object.entries(disarmedToday)
            .map(([k, v]) => `${k} ${v}`)
            .join(' · ')}
        />
      </div>

      <section className="flex flex-col gap-2">
        <h2 className="text-[12px] font-semibold uppercase tracking-wide text-text-dim">
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
        <h2 className="text-[12px] font-semibold uppercase tracking-wide text-text-dim">
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

function Stat({
  label,
  value,
  sub,
  tone,
}: {
  label: string;
  value: number;
  sub?: string;
  tone?: 'info' | 'success';
}) {
  const dot = tone === 'info' ? 'bg-primary' : tone === 'success' ? 'bg-green' : 'bg-white/30';
  return (
    <div className="flex min-w-28 flex-col gap-0.5 rounded-md border border-white/8 bg-white/2 px-3 py-2">
      <div className="flex items-center gap-1.5">
        <span className={`h-1.5 w-1.5 rounded-full ${dot}`} />
        <span className="text-[11px] uppercase text-text-dim">{label}</span>
      </div>
      <span className="text-lg font-semibold tabular-nums text-text">{value}</span>
      {sub && <span className="text-[10px] text-text-dim/70">{sub}</span>}
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
