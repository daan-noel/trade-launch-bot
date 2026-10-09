import { useMemo } from 'react';
import {
  buildEventMarkers,
  buildEventMarkersForEpisodes,
  inspectFromPosition,
  type InspectTarget,
} from 'components/strategy/inspectTarget';
import type { ChartEventMarker } from 'components/token-price-chart';
import { useGetMintEpisodesQuery } from 'store/sharedEndpoints';
import type { RulePositionRecord } from 'types';

/**
 * Chart markers for a token's **whole traded history**: every entered episode on
 * the mint, each with every leg of its exit ladder — not just the one position the
 * view was opened on.
 *
 * A rule re-enters a mint (cooldown + episode cap), and several rules can trade the
 * same mint, so a single-episode overlay silently hides most of what happened on the
 * chart you are staring at. Scoped to `mode`: paper fills are modeled and real ones
 * are money, so overlaying both would state something false.
 *
 * `focus` is the episode the surrounding view describes. It is substituted for its
 * server copy in the union (matched on `focusPositionId`) so the freshest data wins —
 * on the live Console that means the `position_fills` ledger, which is the only source
 * carrying the legs of a position still laddering out. It is also tagged on the chart
 * so it stays identifiable among its siblings.
 *
 * Falls back to the focus episode alone while loading or if the read fails: fewer
 * markers than the truth, never wrong ones. The traded twin of the simulate side's
 * `useSimMintEpisodeOverlay`.
 */
export function useMintEpisodeMarkers({
  mint,
  mode,
  focus,
  focusPositionId,
  focusMode,
  skip = false,
}: {
  mint: string | null | undefined;
  /** `real` | `paper`; anything else reads as `real` (the backend default). */
  mode?: string | null;
  focus: InspectTarget;
  /** Which server episode `focus` replaces; omit for a position with no DB row yet. */
  focusPositionId?: string | null;
  /**
   * `replace` (default) — the modal path: `focus` wins, because it carries the
   * fills ledger. `fallback` — the chart-card path: the server episode already
   * has the exit legs, and `focus` is only the stand-in until that row exists.
   */
  focusMode?: 'replace' | 'fallback';
  skip?: boolean;
}): ChartEventMarker[] {
  const { data: episodes } = useGetMintEpisodesQuery(
    { mint: mint ?? '', mode },
    { skip: skip || !mint },
  );

  return useMemo(() => {
    if (!episodes || episodes.length === 0) return buildEventMarkers(focus);
    const replace = focusMode !== 'fallback';
    const targets = episodes.map((e) =>
      replace && focusPositionId && e.id === focusPositionId ? focus : inspectFromPosition(e),
    );
    // An episode with no DB row yet (an entry still landing) is absent from the read;
    // keep it, or the chart would draw every episode except the one being inspected.
    // In fallback mode the server row already is that episode, so don't draw it twice.
    const onServer =
      focusPositionId != null && episodes.some((e) => e.id === focusPositionId);
    if (!targets.includes(focus) && (replace || !onServer)) targets.push(focus);
    return buildEventMarkersForEpisodes(targets, replace ? focus : null);
  }, [episodes, focus, focusPositionId, focusMode]);
}

/**
 * Chart-card overlay for a live/paper position row. Same markers the inspect
 * modal draws: this episode, plus every other episode on the mint (re-entries
 * and scale-out legs). Falls back to the row alone while that read is in flight.
 */
export function useRulePositionChartOverlay(row: RulePositionRecord): {
  eventMarkers: ChartEventMarker[];
} {
  const focus = useMemo(
    () => inspectFromPosition(row),
    [
      row.id,
      row.mint_address,
      row.symbol,
      row.mode,
      row.target_time,
      row.target_price,
      row.target_tx,
      row.entry_time,
      row.entry_price,
      row.entry_tx,
      row.exit_time,
      row.exit_price,
      row.exit_tx,
      row.exit_reason,
      row.exit_legs,
    ],
  );
  const eventMarkers = useMintEpisodeMarkers({
    mint: row.mint_address,
    mode: row.mode,
    focus,
    focusPositionId: row.id,
    focusMode: 'fallback',
  });
  return { eventMarkers };
}
