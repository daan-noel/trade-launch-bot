import { useState } from 'react';
import {
  useLaddersQuery,
  useArmLadderMutation,
  useCancelLadderMutation,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import { Badge, Banner, Button, Card, Field, IconButton, Input, Select } from '@shared/components/ui';
import { formatUsd } from '@shared/lib/format';
import type { LadderRung, SellLadder } from '@shared/types';

type Group = 'all' | 'dev' | 'bundler';
type Metric = 'market_cap_usd' | 'price_usd';

const METRIC_LABEL: Record<Metric, string> = {
  market_cap_usd: 'Market cap',
  price_usd: 'Price',
};

/** Arm take-profit sell ladders: sell X% of holdings when market cap / price
 *  crosses a threshold. Evaluated by a backend task; a rung only FIRES when the
 *  MANAGE_ENABLED kill switch is on (until then armed ladders wait). Arming and
 *  cancelling are always allowed. */
export function LadderPanel({ mint }: { mint: string }) {
  const { data: ladders = [] } = useLaddersQuery(mint, { skip: !mint });
  const [armLadder, armState] = useArmLadderMutation();
  const [cancelLadder] = useCancelLadderMutation();

  // Optimistic cancel feedback: the DELETE is a near-instant flag flip, but the badge
  // only repaints on refetch, so a click looked dead. Track the ladder being
  // cancelled to disable its button + show a pending affordance until the mutation
  // resolves (invalidates `Ladders` → refetch → badge flips to cancelled).
  const [cancellingId, setCancellingId] = useState<string | null>(null);
  const runCancel = async (id: string) => {
    setCancellingId(id);
    try {
      await cancelLadder(id).unwrap();
    } catch {
      /* keep current status; the query refetch reconciles */
    } finally {
      setCancellingId(null);
    }
  };

  const [group, setGroup] = useState<Group>('all');
  const [metric, setMetric] = useState<Metric>('market_cap_usd');
  const [threshold, setThreshold] = useState(100_000);
  const [pct, setPct] = useState(50);
  const [rungs, setRungs] = useState<LadderRung[]>([]);

  const addRung = () => {
    if (threshold <= 0 || pct <= 0 || pct > 100) return;
    setRungs((r) => [...r, { metric, threshold, pct }].sort((a, b) => a.threshold - b.threshold));
  };
  const removeRung = (i: number) => setRungs((r) => r.filter((_, idx) => idx !== i));

  const onArm = async () => {
    if (rungs.length === 0) return;
    try {
      await armLadder({
        mint,
        selection: group === 'all' ? {} : { role: group },
        rungs,
      }).unwrap();
      setRungs([]);
    } catch {
      /* rendered below */
    }
  };

  const armErr = apiErrorMessage(armState.error);

  return (
    <Card title="Sell ladders (auto take-profit)">
      <div className="space-y-4">
        {/* Builder */}
        <div className="flex flex-wrap items-end gap-3">
          <Field label="Wallet group" className="w-36">
            <Select value={group} onChange={(e) => setGroup(e.target.value as Group)}>
              <option value="all">All wallets</option>
              <option value="dev">Dev only</option>
              <option value="bundler">Bundlers only</option>
            </Select>
          </Field>
          <Field label="When" className="w-36">
            <Select value={metric} onChange={(e) => setMetric(e.target.value as Metric)}>
              <option value="market_cap_usd">Market cap ≥</option>
              <option value="price_usd">Price ≥</option>
            </Select>
          </Field>
          <Field label="USD threshold" className="w-32">
            <Input
              type="number"
              min={0}
              value={threshold}
              onChange={(e) => setThreshold(Math.max(0, Number(e.target.value) || 0))}
            />
          </Field>
          <Field label="Sell %" className="w-24">
            <Input
              type="number"
              min={1}
              max={100}
              value={pct}
              onChange={(e) => setPct(Math.max(1, Math.min(100, Number(e.target.value) || 0)))}
            />
          </Field>
          <Button variant="ghost" icon="plus" onClick={addRung}>
            Add rung
          </Button>
        </div>

        {rungs.length > 0 && (
          <div className="space-y-2">
            <div className="field-label">Pending ladder ({group})</div>
            <ul className="space-y-1">
              {rungs.map((r, i) => (
                <li key={i} className="flex items-center gap-2 text-sm">
                  <Badge tone="info">
                    {METRIC_LABEL[r.metric as Metric]} ≥ {formatUsd(r.threshold)} → sell {r.pct}%
                  </Badge>
                  <IconButton icon="close" label="Remove rung" variant="ghost" size="sm" onClick={() => removeRung(i)} />
                </li>
              ))}
            </ul>
            <Button variant="primary" icon="bolt" onClick={onArm} loading={armState.isLoading}>
              Arm ladder
            </Button>
          </div>
        )}

        {armErr && <Banner tone="bad">{armErr}</Banner>}

        {/* Existing ladders */}
        {ladders.length > 0 && (
          <div className="space-y-2 pt-2">
            <div className="field-label">Ladders</div>
            {ladders.map((l) => (
              <LadderRow
                key={l.id}
                ladder={l}
                busy={cancellingId === l.id}
                onCancel={() => runCancel(l.id)}
              />
            ))}
          </div>
        )}
      </div>
    </Card>
  );
}

function LadderRow({ ladder, busy, onCancel }: { ladder: SellLadder; busy: boolean; onCancel: () => void }) {
  const tone = busy ? 'info' : ladder.status === 'armed' ? 'good' : ladder.status === 'done' ? 'info' : 'neutral';
  const sel = ladder.selection?.role ?? 'all';
  return (
    <div className="flex items-center justify-between gap-3 rounded border border-[var(--color-border)] px-3 py-2">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <Badge tone={tone}>{busy ? 'cancelling…' : ladder.status}</Badge>
        <span className="muted text-xs">{sel}</span>
        {ladder.rungs.map((r, i) => (
          <span key={i} className={`text-xs mono ${r.fired ? 'muted line-through' : ''}`}>
            {formatUsd(r.threshold)}→{r.pct}%
          </span>
        ))}
      </div>
      {ladder.status === 'armed' && (
        <IconButton icon="close" label="Cancel ladder" size="sm" variant="ghost" disabled={busy} onClick={onCancel} />
      )}
    </div>
  );
}
