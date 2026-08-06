import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { buildEventMarkers, type InspectTarget } from 'components/strategy/inspectTarget';
import { StatTile } from 'components/ui/StatTile';
import { rulesHref } from 'lib/strategy/nav';
import { formatCompact } from 'utils/format';
import {
  formatSigned,
  formatSignedPct,
  pctGradeClass,
  signedStatTone,
  signedToneClass,
} from 'lib/signedTone';
import { FloorMintChart } from './FloorMintChart';

export interface FloorDetailFacts {
  mint: string;
  ruleId: string | null;
  ruleName: string | null;
  mode?: string | null;
  status: string;
  entrySol?: number | null;
  entryPrice?: number | null;
  exitPrice?: number | null;
  /** ISO or display age already formatted. */
  holdLabel?: string | null;
  pnlSol?: number | null;
  pnlPct?: number | null;
  mtmSol?: number | null;
  /** Open vs closed chart markers. */
  inspect: InspectTarget;
  /** Fingerprint `volume_ix_patterns` keys for the chart vol/non-vol overlay. */
  flowPatternKeys?: ReadonlySet<string> | null;
  /** Optional action slot (Sell / Trade). */
  actions?: ReactNode;
}

/**
 * Expanded row body for Floor tables — fact strip + mint price chart with
 * entry/exit markers.
 */
export function FloorPositionDetail({
  facts,
  chartHeight = 220,
}: {
  facts: FloorDetailFacts;
  /** Chart height — taller when the detail is shown in a modal vs. an inline row. */
  chartHeight?: number;
}) {
  const markers = buildEventMarkers(facts.inspect);
  const mtmOrPnl = facts.mtmSol ?? facts.pnlSol;
  const pct = facts.pnlPct;

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-6">
        <StatTile label="Status" value={facts.status} size="sm" tone="muted" />
        {facts.mode ? (
          <StatTile label="Mode" value={facts.mode} size="sm" tone="muted" />
        ) : null}
        <StatTile
          label="Entry ◎"
          value={facts.entrySol != null ? formatCompact(facts.entrySol, 3) : '—'}
          size="sm"
        />
        <StatTile
          label="Entry px"
          value={facts.entryPrice != null ? formatCompact(facts.entryPrice, 6) : '—'}
          size="sm"
          tone="muted"
        />
        <StatTile
          label={facts.mtmSol != null ? 'MTM ◎' : 'PnL ◎'}
          value={mtmOrPnl != null ? formatSigned(mtmOrPnl, 3) : '—'}
          size="sm"
          tone={signedStatTone(mtmOrPnl)}
        />
        <StatTile
          label="PnL%"
          value={
            pct != null ? (
              <span className={pctGradeClass(pct)}>{formatSignedPct(pct, 1)}</span>
            ) : (
              '—'
            )
          }
          size="sm"
        />
      </div>

      <div className="flex flex-wrap items-center gap-3 text-[11px]">
        {facts.holdLabel ? (
          <span className="text-text-dim">
            Hold <span className="tabular-nums text-text">{facts.holdLabel}</span>
          </span>
        ) : null}
        {facts.ruleId ? (
          <Link
            to={rulesHref(facts.ruleId)}
            className="font-semibold text-accent hover:underline"
            onClick={(e) => e.stopPropagation()}
          >
            Evidence · {facts.ruleName ?? facts.ruleId.slice(0, 8)}
          </Link>
        ) : null}
        <Link
          to={`/console?mint=${encodeURIComponent(facts.mint)}`}
          className="font-semibold text-accent hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Trade
        </Link>
        {facts.actions}
        {mtmOrPnl != null && (
          <span className={`ml-auto tabular-nums font-semibold ${signedToneClass(mtmOrPnl)}`}>
            {formatSigned(mtmOrPnl, 3)}◎
            {pct != null ? ` (${formatSignedPct(pct, 1)})` : ''}
          </span>
        )}
      </div>

      <FloorMintChart
        mint={facts.mint}
        markers={markers}
        tableId="floor-detail"
        height={chartHeight}
        flowPatternKeys={facts.flowPatternKeys ?? null}
      />
    </div>
  );
}
