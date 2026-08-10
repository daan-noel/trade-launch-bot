import { useEffect } from 'react';
import { useDispatch } from 'react-redux';
import { connectStrategyPositionUpdate, onSseReopen } from 'services/sse';
import { liveApi } from '@live/store/liveEndpoints';
import type { AppDispatch } from '@live/store';

/**
 * Keep an open detail's two chart sources fresh: this position's fill ledger, and
 * the mint's episode list.
 *
 * Both are plain cached GETs, so without this the chart keeps drawing whatever the
 * mint looked like when the modal opened — a scale-out leg on this position, or a
 * whole new re-entry on the mint, would never appear. Both arrive as a
 * `strategy_position_update` frame. Notify over poll: no interval, and the SSE
 * subscription exists only while a detail is mounted.
 *
 * Invalidating (not patching) is right here — the frame carries running aggregates,
 * not a leg's price/tx, and the ledger is the SSOT for those. A reconnect
 * invalidates too: anything that landed inside the gap emits no frame we ever see.
 *
 * The two tags are deliberately driven by different keys. A leg belongs to THIS
 * position, but a new episode belongs to the mint and will carry a position id this
 * component has never heard of — matching on `position_id` alone would miss exactly
 * the case the episode overlay exists for.
 */
export function useLivePositionFills(
  positionId: string | null | undefined,
  mint?: string | null,
): void {
  const dispatch = useDispatch<AppDispatch>();

  useEffect(() => {
    if (!positionId && !mint) return;
    const refresh = () => {
      const tags = [];
      if (positionId) tags.push({ type: 'PositionFills' as const, id: positionId });
      if (mint) tags.push({ type: 'MintEpisodes' as const, id: mint });
      dispatch(liveApi.util.invalidateTags(tags));
    };
    const posH = connectStrategyPositionUpdate((d) => {
      if (d.position_id === positionId || (mint != null && d.mint_address === mint)) refresh();
    });
    const reopenUnsub = onSseReopen(refresh);
    return () => {
      posH.close();
      reopenUnsub();
    };
  }, [dispatch, positionId, mint]);
}
