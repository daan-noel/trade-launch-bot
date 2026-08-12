/**
 * Chart-card adapters for the Console's **live** lanes — what a row contributes
 * to its card when the toolbar's Charts toggle is on.
 *
 * The lanes below History ride `TokenTable` for that toggle, and `TokenTable`
 * asks each row for two things: an overlay (markers) and a header extra (facts).
 * The two lanes answer differently on purpose:
 *
 *   • **Open** has an entry fill, so it draws the same entry marker its inspect
 *     modal does, through the one `InspectTarget` adapter.
 *   • **Waiting / Arms** has no fill at all. A chart marker needs a price, and an
 *     arming episode has none — so these cards carry NO markers and put the
 *     episode's facts (when it armed, how long it waited, how it ended) in the
 *     card header instead. Marking `armed_at` at some invented price would draw
 *     a line the token never traded at.
 */

import { markerRowOverlay, type InspectTarget } from 'components/strategy/inspectTarget';
import { Badge } from 'components/ui/Badge';
import { ElapsedCell } from 'components/table/ElapsedCell';
import { DateCell } from 'components/table/DateCell';
import { holdLabel } from 'lib/holdLabel';
import type { LiveOpenRow } from '@live/slices/liveStatusSlice';

/** Map a live open-position row to the chart's inspect target. No `exitLegs`:
 *  the live lane row carries no fills ledger, and a still-laddering position's
 *  legs come from the modal's own fetch. */
export function inspectFromLiveOpen(r: LiveOpenRow): InspectTarget {
  return {
    mint_address: r.mint_address,
    entryTime: r.entryTime,
    entryPrice: r.entryPrice,
    exitTime: null,
    exitPrice: null,
    exitLabel: r.exitReason,
  };
}

/** Entry markers for an Open lane chart card — the same overlay the row's
 *  inspect modal draws. Hoisted: a fresh closure would remount every card. */
export const liveOpenRowOverlay = markerRowOverlay(inspectFromLiveOpen);

/** How an arming episode ended, as a badge. `null` reason = still waiting. */
export function armEndBadge(reason: string | null | undefined) {
  if (!reason) {
    return (
      <Badge variant="warning" size="sm">
        Waiting
      </Badge>
    );
  }
  // `entered` is the one ending that produced a trade, so it reads as a success
  // rather than as another way the rule passed on the token.
  const variant = reason === 'entered' ? 'success' : 'neutral';
  return (
    <Badge variant={variant} size="sm">
      {ARM_END_LABEL[reason] ?? reason}
    </Badge>
  );
}

/** Wire `end_reason` → what the operator reads. Keys mirror the backend
 *  vocabulary (`ARM_END_REASONS` / `disarm_reason_str`) exactly. */
export const ARM_END_LABEL: Record<string, string> = {
  entered: 'Entered',
  dead: 'Dead',
  migrated: 'Migrated',
  unsatisfiable: 'Unsatisfiable',
  paused: 'Paused',
  duplicate_identity: 'Copycat',
};

/** Facts in an arm card's header, so a card reads like its table row: when the
 *  rule armed, how long it watched, and how the episode ended. `armedAtIso`
 *  drives the live clock while `endedAtIso` is null. */
export function ArmChartCardExtra({
  armedAtIso,
  endedAtIso,
  endReason,
  ruleName,
}: {
  armedAtIso: string;
  endedAtIso?: string | null;
  endReason?: string | null;
  ruleName?: string | null;
}) {
  const waited = holdLabel(armedAtIso, endedAtIso ?? null);
  const armedMs = Date.parse(armedAtIso);
  return (
    <span className="inline-flex flex-wrap items-center gap-2 text-[11px] text-text-dim">
      {armEndBadge(endReason)}
      {ruleName && <span className="text-text-mid">{ruleName}</span>}
      <span>
        armed <DateCell iso={armedAtIso} />
      </span>
      <span className="tabular-nums text-warning">
        {/* A live episode keeps ticking; an ended one is a fixed span. */}
        {endedAtIso ? (waited ?? '—') : Number.isFinite(armedMs) ? <ElapsedCell startMs={armedMs} /> : '—'}
      </span>
    </span>
  );
}
