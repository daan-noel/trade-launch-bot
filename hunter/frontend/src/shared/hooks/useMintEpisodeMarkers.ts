import { useMemo } from 'react';
import {
  buildEventMarkers,
  buildEventMarkersForEpisodes,
  inspectFromPosition,
  type InspectTarget,
} from 'components/strategy/inspectTarget';
import type { ChartEventMarker } from 'components/token-price-chart';
import { useGetMintEpisodesQuery } from 'store/sharedEndpoints';

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
  skip = false,
}: {
  mint: string | null | undefined;
  /** `real` | `paper`; anything else reads as `real` (the backend default). */
  mode?: string | null;
  focus: InspectTarget;
  /** Which server episode `focus` replaces; omit for a position with no DB row yet. */
  focusPositionId?: string | null;
  skip?: boolean;
}): ChartEventMarker[] {
  const { data: episodes } = useGetMintEpisodesQuery(
    { mint: mint ?? '', mode },
    { skip: skip || !mint },
  );

  return useMemo(() => {
    if (!episodes || episodes.length === 0) return buildEventMarkers(focus);
    const targets = episodes.map((e) =>
      focusPositionId && e.id === focusPositionId ? focus : inspectFromPosition(e),
    );
    // An episode with no DB row yet (an entry still landing) is absent from the read;
    // keep it, or the modal would draw every episode except the one being inspected.
    if (!targets.includes(focus)) targets.push(focus);
    return buildEventMarkersForEpisodes(targets, focus);
  }, [episodes, focus, focusPositionId]);
}
