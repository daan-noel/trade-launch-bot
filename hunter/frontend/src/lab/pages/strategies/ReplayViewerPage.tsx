import { useEffect, useMemo, useRef, useState } from 'react';
import { cn } from 'lib/cn';
import { IconButton } from 'components/ui/IconButton';
import { IconButtonGroup } from 'components/ui/IconButtonGroup';
import { PauseIcon, PlayIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { Checkbox } from 'components/ui/Checkbox';
import { Badge } from 'components/ui/Badge';
import { Accordion } from 'components/ui/Accordion';
import { InlineAlert } from 'components/ui/Modal';
import { StatTile } from 'components/ui/StatTile';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import type { ChartEventMarker } from 'components/token-price-chart';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';
import { useInspectReplayMutation } from '@lab/store/labEndpoints';
import {
  eventBody,
  eventKind,
  eventMint,
  type InspectEffect,
  type InspectRequest,
  type InspectStep,
} from '@lab/services/replayInspect';

/** Effect chip color by kind. */
const EFFECT_TONE: Record<InspectEffect['effect'], string> = {
  SubmitBuy: 'text-buy border-buy/30 bg-buy/10',
  SubmitSell: 'text-sell border-sell/30 bg-sell/10',
  PositionUpdate: 'text-info border-info/30 bg-info/10',
  ArmedChanged: 'text-accent border-accent/30 bg-accent/10',
};

/** Event-kind tone for the timeline row marker. */
function eventTone(kind: string): string {
  switch (kind) {
    case 'Trade':
      return 'text-secondary';
    case 'Tick':
      return 'text-text-dim/60';
    case 'FillConfirmed':
      return 'text-green';
    case 'FillFailed':
      return 'text-red';
    case 'TokenCreated':
    case 'FirstSlotSettled':
      return 'text-accent';
    default:
      return 'text-text-mid';
  }
}

/** Fixed-width `HH:mm:ss.SSS` from an RFC3339 instant. */
function fmtTime(iso: string | null | undefined): string {
  if (!iso) return '—';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '—';
  const p = (n: number, w = 2) => String(n).padStart(w, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

/** One-line summary of a logged event for the timeline row. */
function eventSummary(step: InspectStep): string {
  const kind = eventKind(step.event);
  const b = eventBody(step.event);
  switch (kind) {
    case 'Trade': {
      const t = (b.trade ?? {}) as Record<string, unknown>;
      const side = String(t.side ?? '?').toLowerCase();
      return `${side} ${num(t.sol)}◎ @ ${num(t.price, 6)}`;
    }
    case 'Tick':
      return 'tick';
    case 'TokenCreated':
      return 'created';
    case 'FirstSlotSettled':
      return `first-slot buy ${lamportsSol(b.buy_lamports)}◎ / sell ${lamportsSol(b.sell_lamports)}◎`;
    case 'FillConfirmed': {
      const f = (b.fill ?? {}) as Record<string, unknown>;
      return `fill @ ${num(f.price, 6)} (${num(f.sol)}◎)`;
    }
    case 'FillFailed':
      return `fill failed: ${String(b.reason ?? '')}`;
    case 'Migrated':
      return 'migrated';
    case 'ManualClose':
      return 'manual close';
    default:
      return '';
  }
}

function num(v: unknown, dp = 4): string {
  return typeof v === 'number' ? v.toFixed(dp).replace(/\.?0+$/, '') : '—';
}
function lamportsSol(v: unknown): string {
  return typeof v === 'number' ? (v / 1e9).toFixed(3) : '—';
}

/** Compact one-line description of one effect. */
function effectSummary(fx: InspectEffect): string {
  switch (fx.effect) {
    case 'SubmitBuy':
      return `buy ${lamportsSol(fx.lamports)}◎`;
    case 'SubmitSell':
      return `sell · ${fx.reason ?? ''}`;
    case 'PositionUpdate':
      return `${fx.status ?? ''}${fx.reason ? ` · ${fx.reason}` : ''}${fx.fill ? ` @ ${fx.fill.price.toFixed(6)}` : ''}`;
    case 'ArmedChanged':
      return String(fx.state ?? '');
  }
}

export function ReplayViewerPage() {
  const [dir, setDir] = useState('');
  const [date, setDate] = useState('');
  const [mint, setMint] = useState('');
  const [since, setSince] = useState('');
  const [until, setUntil] = useState('');
  const [syntheticTicks, setSyntheticTicks] = useState(true);
  const [activeOnly, setActiveOnly] = useState(false);
  const [maxSteps, setMaxSteps] = useState(10000);

  const [inspect, { data: run, isLoading, error }] = useInspectReplayMutation();
  const runErr = apiErrorMessage(error, 'Replay failed');

  const steps = useMemo(() => run?.steps ?? [], [run]);
  const [sel, setSel] = useState(0);
  const [playing, setPlaying] = useState(false);

  // Reset selection + stop playing when a new run arrives.
  useEffect(() => {
    setSel(0);
    setPlaying(false);
  }, [run]);

  // Playback: advance one step at a fixed cadence; stop at the end.
  useEffect(() => {
    if (!playing || steps.length === 0) return;
    const id = setInterval(() => {
      setSel((s) => {
        if (s >= steps.length - 1) {
          setPlaying(false);
          return s;
        }
        return s + 1;
      });
    }, 600);
    return () => clearInterval(id);
  }, [playing, steps.length]);

  // Keep the selected row scrolled into view as playback / stepping moves it.
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-step="${sel}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  }, [sel]);

  const selStep = steps[sel] ?? null;

  const runInspect = () => {
    const toIso = (local: string) => (local ? new Date(`${local}Z`).toISOString() : undefined);
    const req: InspectRequest = {
      dir: dir.trim() || undefined,
      date: date.trim() || undefined,
      mint: mint.trim() || undefined,
      since: toIso(since),
      until: toIso(until),
      synthetic_ticks: syntheticTicks,
      active_only: activeOnly,
      max_steps: maxSteps,
    };
    void inspect(req);
  };

  // The token the chart focuses on: the mint filter, else the selected step's mint.
  const focusMint = (mint.trim() || (selStep ? eventMint(selStep.event) : null) || '').trim();
  const { data: detail, isFetching: detailLoading } = useGetTokenDetailQuery(focusMint, {
    skip: !focusMint,
  });

  // Entry/exit markers for the focused token, derived from the run's PositionUpdate
  // fills. The selected step's timestamp is the "cursor" — shown in the header and
  // (when it carries a price) added as an extra marker so it reads on the chart.
  const markers = useMemo<ChartEventMarker[]>(() => {
    if (!focusMint) return [];
    const out: ChartEventMarker[] = [];
    for (const step of steps) {
      for (const fx of step.effects) {
        if (fx.effect !== 'PositionUpdate' || fx.mint !== focusMint || !fx.fill) continue;
        const isExit = fx.status === 'End' || fx.status === 'ExitPending' || fx.status === 'ExitUnconfirmed';
        out.push({
          kind: isExit ? 'exit' : 'entry',
          time: fx.fill.at,
          priceInSol: fx.fill.price,
          label: isExit ? `Exit${fx.reason ? ` · ${fx.reason}` : ''}` : 'Entry',
        });
      }
    }
    return out;
  }, [steps, focusMint]);

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center gap-2.5">
        <h1 className="text-lg font-semibold text-text">Replay viewer</h1>
        <span className="text-[12px] text-text-dim">
          Re-run the engine over a recorded live event log — every decision, reproduced offline.
        </span>
      </div>

      <Accordion title="Load a log slice" defaultOpen={!run}>
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-end gap-3">
            <Field label="Dir" hint="blank = EVENT_LOG_DIR" className="w-[180px]">
              <Input value={dir} onChange={(e) => setDir(e.target.value)} placeholder="event_log" />
            </Field>
            <Field label="Date" hint="one day-file" className="w-[150px]">
              <Input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
            </Field>
            <Field label="Mint" hint="focus one token" className="w-[280px]">
              <Input value={mint} onChange={(e) => setMint(e.target.value)} placeholder="all tokens" />
            </Field>
            <Field label="Since" hint="UTC" className="w-[210px]">
              <Input type="datetime-local" value={since} onChange={(e) => setSince(e.target.value)} />
            </Field>
            <Field label="Until" hint="UTC" className="w-[210px]">
              <Input type="datetime-local" value={until} onChange={(e) => setUntil(e.target.value)} />
            </Field>
            <Field label="Max steps" className="w-[120px]">
              <Input
                type="number"
                min={1}
                value={maxSteps}
                onChange={(e) => setMaxSteps(Math.max(1, Number(e.target.value) || 1))}
              />
            </Field>
            <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
              <Checkbox checked={syntheticTicks} onChange={(e) => setSyntheticTicks(e.target.checked)} />
              <span>synthetic ticks</span>
            </label>
            <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
              <Checkbox checked={activeOnly} onChange={(e) => setActiveOnly(e.target.checked)} />
              <span>active rules only</span>
            </label>
            <IconButton
              variant="primary"
              size="lg"
              onClick={runInspect}
              disabled={isLoading}
              label={isLoading ? 'Replaying…' : 'Replay'}
              title={isLoading ? 'Replaying…' : 'Replay'}
            >
              {isLoading ? <SpinnerIcon /> : <PlayIcon />}
            </IconButton>
          </div>
          <p className="text-[11px] text-text-dim/70">
            Rules load from Postgres (the log omits them), so the replay runs against the
            <em> current</em> rule set. `mint`/`since`/`until` narrow only the output — the whole
            log is still folded (cross-token caps honored); `date` narrows which files load.
          </p>
        </div>
      </Accordion>

      {runErr && <InlineAlert variant="error">{runErr}</InlineAlert>}

      {run && (
        <>
          <div className="flex flex-wrap gap-2">
            <StatTile label="Rules" value={String(run.rules_loaded)} />
            <StatTile label="Fingerprints" value={String(run.fingerprints_loaded)} />
            <StatTile label="Logged events" value={run.logged_events.toLocaleString()} />
            <StatTile label="Synthetic ticks" value={run.synthetic_ticks.toLocaleString()} />
            <StatTile label="Replayed" value={run.events_replayed.toLocaleString()} />
            <StatTile label="Steps" value={run.steps_returned.toLocaleString()} />
            {run.truncated && (
              <div className="flex items-center">
                <Badge variant="warning">truncated at max steps</Badge>
              </div>
            )}
          </div>
          <p className="font-mono text-[11px] text-text-dim">
            {run.dir} · {run.files.length ? run.files.join(', ') : 'no day-files'}
          </p>

          {steps.length === 0 ? (
            <div className="rounded-md border border-white/10 bg-surface p-3 text-sm text-text-dim">
              No steps matched the filter (the log folded {run.events_replayed.toLocaleString()} events).
            </div>
          ) : (
            <div className="grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
              {/* --- Decision timeline (left) --- */}
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-1.5">
                  <IconButtonGroup className="gap-1.5">
                  <IconButton
                    size="md"
                    variant="subtle"
                    onClick={() => setSel(0)}
                    disabled={sel === 0}
                    title="Skip to start"
                    aria-label="Skip to start"
                  >
                    <span className="inline-flex -space-x-2">
                      <PlayIcon className="rotate-180 scale-75" />
                      <PlayIcon className="rotate-180 scale-75" />
                    </span>
                  </IconButton>
                  <IconButton
                    size="md"
                    variant="subtle"
                    onClick={() => setSel((s) => Math.max(0, s - 1))}
                    disabled={sel === 0}
                    title="Step back"
                    aria-label="Step back"
                  >
                    <PlayIcon className="rotate-180" />
                  </IconButton>
                  <IconButton
                    size="md"
                    variant={playing ? 'primary' : 'subtle'}
                    onClick={() => setPlaying((p) => !p)}
                    title={playing ? 'Pause' : 'Play'}
                    aria-label={playing ? 'Pause' : 'Play'}
                  >
                    {playing ? <PauseIcon /> : <PlayIcon />}
                  </IconButton>
                  <IconButton
                    size="md"
                    variant="subtle"
                    onClick={() => setSel((s) => Math.min(steps.length - 1, s + 1))}
                    disabled={sel >= steps.length - 1}
                    title="Step forward"
                    aria-label="Step forward"
                  >
                    <PlayIcon />
                  </IconButton>
                  <IconButton
                    size="md"
                    variant="subtle"
                    onClick={() => setSel(steps.length - 1)}
                    disabled={sel >= steps.length - 1}
                    title="Skip to end"
                    aria-label="Skip to end"
                  >
                    <span className="inline-flex -space-x-2">
                      <PlayIcon className="scale-75" />
                      <PlayIcon className="scale-75" />
                    </span>
                  </IconButton>
                  </IconButtonGroup>
                  <span className="ml-2 font-mono text-[12px] text-text-dim tabular-nums">
                    step {sel + 1} / {steps.length}
                  </span>
                  <span className="ml-auto font-mono text-[12px] text-primary tabular-nums">
                    cursor {fmtTime(selStep?.at)}
                  </span>
                </div>
                <div
                  ref={listRef}
                  className="max-h-[65vh] overflow-y-auto rounded-md border border-white/10 bg-surface"
                >
                  {steps.map((step, i) => {
                    const kind = eventKind(step.event);
                    return (
                      <button
                        key={step.seq}
                        data-step={i}
                        type="button"
                        onClick={() => setSel(i)}
                        className={cn(
                          'flex w-full items-center gap-2 border-b border-white/5 px-2 py-1 text-left text-[12px] hover:bg-white/5',
                          i === sel && 'bg-primary/12',
                        )}
                      >
                        <span className="w-16 shrink-0 font-mono text-[10px] text-text-dim tabular-nums">
                          {fmtTime(step.at)}
                        </span>
                        <span className={cn('w-24 shrink-0 font-mono text-[11px]', eventTone(kind))}>{kind}</span>
                        <span className="flex-1 truncate text-text-dim">{eventSummary(step)}</span>
                        {step.effects.map((fx, j) => (
                          <span
                            key={j}
                            className={cn('shrink-0 rounded border px-1 font-mono text-[9px]', EFFECT_TONE[fx.effect])}
                            title={effectSummary(fx)}
                          >
                            {fx.effect}
                          </span>
                        ))}
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* --- Selected step + token chart (right) --- */}
              <div className="flex flex-col gap-3">
                {selStep && (
                  <div className="rounded-md border border-white/10 bg-surface p-2.5">
                    <div className="mb-1.5 flex items-center gap-2">
                      <span className="text-[12px] font-semibold text-text">Step {selStep.seq}</span>
                      <span className={cn('font-mono text-[11px]', eventTone(eventKind(selStep.event)))}>
                        {eventKind(selStep.event)}
                      </span>
                      <span className="font-mono text-[11px] text-text-dim">{fmtTime(selStep.at)}</span>
                    </div>
                    {selStep.effects.length === 0 ? (
                      <p className="text-[11px] text-text-dim">no effects</p>
                    ) : (
                      <ul className="flex flex-col gap-1">
                        {selStep.effects.map((fx, j) => (
                          <li key={j} className="flex items-center gap-2 text-[12px]">
                            <span className={cn('rounded border px-1 font-mono text-[10px]', EFFECT_TONE[fx.effect])}>
                              {fx.effect}
                            </span>
                            <span className="text-text-mid">{effectSummary(fx)}</span>
                            {fx.mint && (
                              <span className="font-mono text-[10px] text-text-dim" title={fx.mint}>
                                {fx.mint.slice(0, 6)}…
                              </span>
                            )}
                          </li>
                        ))}
                      </ul>
                    )}
                    <pre className="mt-2 max-h-40 overflow-auto rounded bg-black/20 p-2 font-mono text-[10px] leading-relaxed text-text-dim">
                      {JSON.stringify(selStep.event, null, 2)}
                    </pre>
                  </div>
                )}

                {focusMint ? (
                  <div className="flex flex-col gap-2">
                    <TokenDetailPanel detail={detail ?? null} loading={detailLoading} error={null} />
                    <TokenTradeChart tableId="replay_inspect_trades" detail={detail ?? null} eventMarkers={markers} />
                  </div>
                ) : (
                  <div className="rounded-md border border-white/10 bg-surface p-3 text-sm text-text-dim">
                    Set a Mint filter (or select a step that touches a token) to show its price chart with the
                    replay's entry/exit markers.
                  </div>
                )}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** Small labelled field (local — mirrors the sweep form's caption style). */
function Field({
  label,
  hint,
  className,
  children,
}: {
  label: string;
  hint?: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={cn('flex flex-col gap-1', className)}>
      <span className="flex items-center gap-1 text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
        {label}
        {hint && <span className="font-normal normal-case tracking-normal text-text-dim/45">{hint}</span>}
      </span>
      {children}
    </div>
  );
}
