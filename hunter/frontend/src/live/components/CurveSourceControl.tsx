import { cn } from 'lib/cn';
import {
  useGetCurveSourceQuery,
  useSetCurveSourceMutation,
} from '@live/store/liveEndpoints';

/** The only two transports the backend accepts — mirrors `curve_source.rs`. */
const SOURCES = [
  {
    value: 'grpc',
    label: 'gRPC',
    title:
      'Yellowstone LaserStream — the direct Helius subscription. The default, and the ' +
      'only transport AMM pool traffic ever uses.',
  },
  {
    value: 'nats',
    label: 'NATS',
    title:
      'The NATS relay. Selecting this with no NATS_URL configured is accepted and ' +
      'persisted, but the ingest adapter keeps the curve on gRPC and logs a warning — ' +
      'the feed is never pointed at a transport that cannot run.',
  },
] as const;

/**
 * Live-only switch for which transport carries **bonding-curve** traffic.
 *
 * Sits beside {@link LiveModeControl} because it is the same kind of control — an
 * operator switch over the running feed, persisted then published — and it had no
 * UI at all: the routes shipped and the only way to move the feed was a hand-rolled
 * PUT. Owns its query + mutation so the lab build (no ingest to switch) never
 * imports them.
 *
 * The switch is not a restart. The settings snapshot is published after the write
 * and the ingest adapter is subscribed to it, so the new source connects while the
 * old one drains and the dedupe ring absorbs the overlap. AMM pool traffic is
 * unaffected either way — it always rides the gRPC subscription, whose filter is
 * keyed on the tracked pool PDAs, so open positions never lose their feed across a
 * switch.
 */
export function CurveSourceControl() {
  const { data: source, isLoading } = useGetCurveSourceQuery();
  const [setSource, { isLoading: switching }] = useSetCurveSourceMutation();

  // No optimistic paint: this moves the live trade feed, so the pressed segment has
  // to be what the server persisted, not what was clicked. A failed write must not
  // leave the UI claiming a source the next boot would not restore.
  const select = async (value: string) => {
    if (value === source || switching) return;
    try {
      await setSource(value).unwrap();
    } catch {
      /* the readback stays on the persisted value */
    }
  };

  return (
    <>
      <div className="hidden h-5 w-px bg-white/8 sm:block" aria-hidden />
      <div
        role="group"
        aria-label="Curve feed transport"
        title="Which transport carries bonding-curve traffic. AMM pool traffic always rides gRPC."
        className="inline-flex items-center gap-1 rounded-full border border-white/10 bg-surface px-1 py-0.5"
      >
        <span className="pl-1.5 pr-0.5 text-[10px] font-bold uppercase tracking-wider text-text-dim">
          curve
        </span>
        {SOURCES.map((s) => {
          const active = source === s.value;
          return (
            <button
              key={s.value}
              type="button"
              aria-pressed={active}
              // Disabled only while the value is unknown — a mid-flight switch keeps
              // the group live so the other segment stays readable.
              disabled={isLoading}
              title={s.title}
              onClick={() => void select(s.value)}
              className={cn(
                'min-h-6 rounded-full px-2 text-[10px] font-bold uppercase tracking-wider transition-colors',
                active
                  ? 'bg-primary/18 text-primary'
                  : 'text-text-dim hover:bg-white/6 hover:text-text',
                switching && 'opacity-60',
              )}
            >
              {s.label}
            </button>
          );
        })}
      </div>
    </>
  );
}
