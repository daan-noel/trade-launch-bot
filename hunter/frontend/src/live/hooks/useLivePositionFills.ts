import { useEffect } from 'react';
import { useDispatch } from 'react-redux';
import { connectStrategyPositionUpdate, onSseReopen } from 'services/sse';
import { liveApi } from '@live/store/liveEndpoints';
import type { AppDispatch } from '@live/store';

/**
 * Keep one position's fill ledger fresh while its detail is open.
 *
 * A scale-out leg lands as a `strategy_position_update` frame; the ledger read is
 * a plain cached GET, so without this the chart keeps drawing the exit arrows the
 * position had when the modal opened. Notify over poll: no interval, and the SSE
 * subscription exists only while a detail is mounted.
 *
 * Invalidating (not patching) is right here — the frame carries running
 * aggregates, not the leg's price/tx, and the ledger is the SSOT for those.
 * A reconnect invalidates too: a leg that landed inside the gap emits no frame we
 * ever see.
 */
export function useLivePositionFills(positionId: string | null | undefined): void {
  const dispatch = useDispatch<AppDispatch>();

  useEffect(() => {
    if (!positionId) return;
    const refresh = () =>
      dispatch(liveApi.util.invalidateTags([{ type: 'PositionFills', id: positionId }]));
    const posH = connectStrategyPositionUpdate((d) => {
      if (d.position_id === positionId) refresh();
    });
    const reopenUnsub = onSseReopen(refresh);
    return () => {
      posH.close();
      reopenUnsub();
    };
  }, [dispatch, positionId]);
}
