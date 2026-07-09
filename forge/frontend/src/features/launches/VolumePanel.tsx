import { useState } from 'react';
import {
  useVolumeBotsQuery,
  useStartVolumeBotMutation,
  usePauseVolumeBotMutation,
  useResumeVolumeBotMutation,
  useStopVolumeBotMutation,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import { Badge, Banner, Button, Card, Field, Input, Select } from '@shared/components/ui';
import { formatSol } from '@shared/lib/format';
import type { VolumeBot, VolumeConfig } from '@shared/types';

type Group = 'trading' | 'dev' | 'bundler';

const num = (v: string, min = 0) => Math.max(min, Number(v) || 0);

/** Author + control volume-making bots: a bot cycles a jittered buy (+ optional
 *  sell-back) across a wallet group on an interval, bounded by a hard SOL budget.
 *  Starting a bot is always allowed; it only trades once the MANAGE_ENABLED kill
 *  switch is on (until then a `running` bot sits idle). */
export function VolumePanel({ mint }: { mint: string }) {
  const { data: bots = [] } = useVolumeBotsQuery(mint, { skip: !mint, pollingInterval: 10_000 });
  const [startBot, startState] = useStartVolumeBotMutation();
  const [pauseBot] = usePauseVolumeBotMutation();
  const [resumeBot] = useResumeVolumeBotMutation();
  const [stopBot] = useStopVolumeBotMutation();

  const [group, setGroup] = useState<Group>('trading');
  const [cfg, setCfg] = useState<VolumeConfig>({
    buy_sol_min: 0.01,
    buy_sol_max: 0.05,
    interval_secs_min: 30,
    interval_secs_max: 120,
    sell_back_pct: 100,
    budget_sol: 1,
    max_cycles: null,
  });
  const set = (patch: Partial<VolumeConfig>) => setCfg((c) => ({ ...c, ...patch }));

  const invalid =
    cfg.buy_sol_min <= 0 ||
    cfg.buy_sol_max < cfg.buy_sol_min ||
    cfg.interval_secs_min <= 0 ||
    cfg.interval_secs_max < cfg.interval_secs_min ||
    cfg.sell_back_pct < 0 ||
    cfg.sell_back_pct > 100 ||
    cfg.budget_sol < cfg.buy_sol_max;

  const onStart = async () => {
    if (invalid) return;
    try {
      await startBot({ mint, selection: { role: group }, config: cfg }).unwrap();
    } catch {
      /* rendered below */
    }
  };

  const startErr = apiErrorMessage(startState.error);

  return (
    <Card title="Volume bots (auto buy/sell loop)">
      <div className="space-y-4">
        {/* Builder */}
        <div className="flex flex-wrap items-end gap-3">
          <Field label="Wallet group" className="w-36">
            <Select value={group} onChange={(e) => setGroup(e.target.value as Group)}>
              <option value="trading">Trading pool</option>
              <option value="dev">Dev</option>
              <option value="bundler">Bundlers</option>
            </Select>
          </Field>
          <Field label="Buy SOL (min)" className="w-28">
            <Input type="number" min={0} step="0.001" value={cfg.buy_sol_min}
              onChange={(e) => set({ buy_sol_min: num(e.target.value) })} />
          </Field>
          <Field label="Buy SOL (max)" className="w-28">
            <Input type="number" min={0} step="0.001" value={cfg.buy_sol_max}
              onChange={(e) => set({ buy_sol_max: num(e.target.value) })} />
          </Field>
          <Field label="Interval s (min)" className="w-28">
            <Input type="number" min={1} value={cfg.interval_secs_min}
              onChange={(e) => set({ interval_secs_min: num(e.target.value, 1) })} />
          </Field>
          <Field label="Interval s (max)" className="w-28">
            <Input type="number" min={1} value={cfg.interval_secs_max}
              onChange={(e) => set({ interval_secs_max: num(e.target.value, 1) })} />
          </Field>
          <Field label="Sell back %" className="w-24">
            <Input type="number" min={0} max={100} value={cfg.sell_back_pct}
              onChange={(e) => set({ sell_back_pct: Math.min(100, num(e.target.value)) })} />
          </Field>
          <Field label="Budget SOL" className="w-28">
            <Input type="number" min={0} step="0.01" value={cfg.budget_sol}
              onChange={(e) => set({ budget_sol: num(e.target.value) })} />
          </Field>
          <Field label="Max cycles" className="w-28">
            <Input type="number" min={0} placeholder="∞" value={cfg.max_cycles ?? ''}
              onChange={(e) => set({ max_cycles: e.target.value ? num(e.target.value, 1) : null })} />
          </Field>
          <Button variant="primary" onClick={onStart} loading={startState.isLoading} disabled={invalid}>
            Start bot
          </Button>
        </div>

        <p className="text-xs muted">
          Sell back 0% = accumulate only (buy-only volume). The bot stops itself when the SOL
          budget or max cycles is reached, and only trades while token management is enabled.
        </p>

        {startErr && <Banner tone="bad">{startErr}</Banner>}

        {/* Running/past bots */}
        {bots.length > 0 && (
          <div className="space-y-2 pt-2">
            <div className="field-label">Bots</div>
            {bots.map((b) => (
              <BotRow
                key={b.id}
                bot={b}
                onPause={() => pauseBot(b.id)}
                onResume={() => resumeBot(b.id)}
                onStop={() => stopBot(b.id)}
              />
            ))}
          </div>
        )}
      </div>
    </Card>
  );
}

function BotRow({
  bot,
  onPause,
  onResume,
  onStop,
}: {
  bot: VolumeBot;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
}) {
  const tone = bot.status === 'running' ? 'good' : bot.status === 'paused' ? 'info' : 'neutral';
  const c = bot.config;
  return (
    <div className="flex items-center justify-between gap-3 rounded border border-[var(--color-border)] px-3 py-2">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <Badge tone={tone}>{bot.status}</Badge>
        <span className="muted text-xs">{bot.selection?.role ?? 'trading'}</span>
        <span className="text-xs mono">
          {c.buy_sol_min}–{c.buy_sol_max} SOL / {c.interval_secs_min}–{c.interval_secs_max}s · sell {c.sell_back_pct}%
        </span>
        <span className="text-xs muted">
          {bot.cycles_done} cycles · spent {formatSol(bot.spent_quote)} / {c.budget_sol} SOL · vol {formatSol(bot.volume_quote)}
        </span>
        {bot.last_error && <span className="text-xs text-[var(--color-bad)]">{bot.last_error}</span>}
      </div>
      <div className="flex items-center gap-2">
        {bot.status === 'running' && (
          <Button size="sm" variant="ghost" onClick={onPause}>Pause</Button>
        )}
        {bot.status === 'paused' && (
          <Button size="sm" variant="ghost" onClick={onResume}>Resume</Button>
        )}
        {bot.status !== 'stopped' && (
          <Button size="sm" variant="ghost" onClick={onStop}>Stop</Button>
        )}
      </div>
    </div>
  );
}
