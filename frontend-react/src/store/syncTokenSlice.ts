import { createSlice, type PayloadAction } from '@reduxjs/toolkit';
import type { TokenDetailRecord, TradeRecord } from '../types';

/** A single token's sync outcome. */
export type SyncResultItem = { mint: string; ok: boolean; error?: string };

/** A token that synced successfully, paired with its trades. */
export type SyncedToken = { token: TokenDetailRecord; trades: TradeRecord[] };

interface SyncTokenState {
  /** Per-mint ok/fail results, accumulated across sync runs. */
  results: SyncResultItem[];
  /** Successfully synced tokens with their trades (drives the chart + trades table). */
  syncedTokens: SyncedToken[];
  /** Mint of the selected synced-token row, or null. */
  selectedMint: string | null;
  /**
   * Every mint address synced this session, deduped in first-seen order. A
   * lightweight cache (just the strings) so the user can review/reload what
   * they've synced even after the heavier results/syncedTokens are gone.
   */
  syncedMints: string[];
}

const initialState: SyncTokenState = {
  results: [],
  syncedTokens: [],
  selectedMint: null,
  syncedMints: [],
};

const syncTokenSlice = createSlice({
  name: 'syncToken',
  initialState,
  reducers: {
    /** Reset all accumulated sync output and the mint cache. */
    clearSyncOutput(state) {
      state.results = [];
      state.syncedTokens = [];
      state.selectedMint = null;
      state.syncedMints = [];
    },
    /**
     * Merge a completed (or partially aborted) sync run into the existing
     * output instead of replacing it: re-syncing a mint updates its row in
     * place, new mints are appended, and the mint cache grows.
     */
    mergeSyncOutput(
      state,
      action: PayloadAction<{
        results: SyncResultItem[];
        syncedTokens: SyncedToken[];
      }>,
    ) {
      // Merge per-mint results: incoming overrides existing, else appended.
      for (const r of action.payload.results) {
        const idx = state.results.findIndex((x) => x.mint === r.mint);
        if (idx >= 0) state.results[idx] = r;
        else state.results.push(r);
      }
      // Merge successfully synced tokens by mint address (same dedup rule).
      for (const t of action.payload.syncedTokens) {
        const idx = state.syncedTokens.findIndex(
          (x) => x.token.mint_address === t.token.mint_address,
        );
        if (idx >= 0) state.syncedTokens[idx] = t;
        else state.syncedTokens.push(t);
      }
      // Grow the mint cache (deduped, first-seen order).
      for (const r of action.payload.results) {
        if (!state.syncedMints.includes(r.mint)) state.syncedMints.push(r.mint);
      }
      // Keep the current selection if it still exists; otherwise select the
      // first newly synced token, falling back to the first synced overall.
      const stillValid =
        state.selectedMint != null &&
        state.syncedTokens.some((t) => t.token.mint_address === state.selectedMint);
      if (!stillValid) {
        state.selectedMint =
          action.payload.syncedTokens[0]?.token.mint_address ??
          state.syncedTokens[0]?.token.mint_address ??
          null;
      }
    },
    setSelectedMint(state, action: PayloadAction<string | null>) {
      state.selectedMint = action.payload;
    },
  },
});

export const { clearSyncOutput, mergeSyncOutput, setSelectedMint } = syncTokenSlice.actions;

export default syncTokenSlice.reducer;
