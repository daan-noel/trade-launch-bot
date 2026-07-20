import { memo } from 'react';
import { skipToken } from '@reduxjs/toolkit/query/react';
import { Accordion } from 'components/ui/Accordion';
import { Badge } from 'components/ui/Badge';
import { IconButton } from 'components/ui/IconButton';
import { RefreshIcon, SpinnerIcon } from 'components/ui/icons';
import { useGetArmedHistoryQuery } from '@live/store/liveEndpoints';

const shortMint = (a: string) => `${a.slice(0, 4)}…${a.slice(-4)}`;

const fmtTime = (iso: string) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleTimeString();
};

/** Seconds a candidate armed on the feed before it left un-fired. */
const watchedSecs = (armed: string, ended: string) => {
  const a = new Date(armed).getTime();
  const e = new Date(ended).getTime();
  return Number.isNaN(a) || Number.isNaN(e) ? null : Math.max(0, Math.round((e - a) / 1000));
};

/**
 * Current-run "armed but never fired" candidates for the selected rule — tokens
 * that matched and armed on the live feed but whose entry trigger never came
 * (window closed, armer cap evicted them, or the rule was stopped). Read from the
 * in-memory runtime cache (these rows are deleted on drop, so there's no DB
 * history); the list resets when a fresh run starts. Polled while open so it stays
 * current. Collapsed by default — a convenience view, not a primary table.
 */
export const ArmedHistoryPanel = memo(function ArmedHistoryPanel({
  strategy,
  selectedRuleId,
}: {
  /** Strategy path segment — short alias (`tpsl2`) or canonical id (`swing_1`). */
  strategy: string;
  selectedRuleId: string | null;
}) {
  const { data, isFetching, refetch } = useGetArmedHistoryQuery(
    selectedRuleId ? { strategy, ruleId: selectedRuleId } : skipToken,
    { pollingInterval: 10_000, refetchOnMountOrArgChange: true },
  );
  const records = data ?? [];
  // Newest first — the most recent misses are the interesting ones.
  const rows = [...records].reverse();

  return (
    <Accordion
      className="mt-1"
      padding="sm"
      defaultOpen={false}
      title="Armed · never fired"
      badge={
        <Badge variant="info" size="sm" className="font-mono font-normal">
          {records.length}
        </Badge>
      }
    >
      <div className="mb-2 flex items-center gap-2">
        <span className="text-[11px] text-text-dim">
          Current run only — candidates that armed but whose entry trigger never fired.
        </span>
        <span className="flex-1" />
        <IconButton
          variant="ghost"
          size="md"
          disabled={isFetching}
          onClick={() => void refetch()}
          title={isFetching ? 'Refreshing…' : 'Refresh'}
          aria-label={isFetching ? 'Refreshing…' : 'Refresh'}
        >
          {isFetching ? <SpinnerIcon /> : <RefreshIcon />}
        </IconButton>
      </div>

      {rows.length === 0 ? (
        <p className="py-2 text-[12px] text-text-dim">No armed-but-unfired candidates this run.</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-[12px]">
            <thead>
              <tr className="text-left text-text-dim">
                <th className="py-1 pr-4 font-medium">Mint</th>
                <th className="py-1 pr-4 font-medium">Armed</th>
                <th className="py-1 pr-4 font-medium">Ended</th>
                <th className="py-1 font-medium">Watched</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => {
                const secs = watchedSecs(r.armed_at, r.ended_at);
                return (
                  <tr key={r.position_id} className="border-t border-white/5">
                    <td className="py-1 pr-4 font-mono" title={r.mint_address}>
                      {shortMint(r.mint_address)}
                    </td>
                    <td className="py-1 pr-4 font-mono text-text-dim">{fmtTime(r.armed_at)}</td>
                    <td className="py-1 pr-4 font-mono text-text-dim">{fmtTime(r.ended_at)}</td>
                    <td className="py-1 font-mono text-text-dim">{secs == null ? '—' : `${secs}s`}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </Accordion>
  );
});
