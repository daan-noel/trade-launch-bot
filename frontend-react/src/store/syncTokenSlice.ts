import { createSlice, type PayloadAction } from '@reduxjs/toolkit';
import type { TokenDetailRecord, TradeRecord } from '../types';

/** A single token's sync outcome. */
export type SyncResultItem = { mint: string; ok: boolean; error?: string };

/** A token that synced successfully, paired with its trades. */
export type SyncedToken = { token: TokenDetailRecord; trades: TradeRecord[] };

interface SyncTokenState {
  /** Per-mint ok/fail results from the last sync run. */
  results: SyncResultItem[];
  /** Successfully synced tokens with their trades (drives the chart + trades table). */
  syncedTokens: SyncedToken[];
  /** Mint of the selected synced-token row, or null. */
  selectedMint: string | null;
}

const initialState: SyncTokenState = {
  results: [],
  syncedTokens: [],
  selectedMint: null,
};

const syncTokenSlice = createSlice({
  name: 'syncToken',
  initialState,
  reducers: {
    /** Clear previous output when a new sync run starts. */
    clearSyncOutput(state) {
      state.results = [];
      state.syncedTokens = [];
      state.selectedMint = null;
    },
    /** Store the completed (or partially aborted) sync output in one shot. */
    setSyncOutput(
      state,
      action: PayloadAction<{
        results: SyncResultItem[];
        syncedTokens: SyncedToken[];
        selectedMint: string | null;
      }>,
    ) {
      state.results = action.payload.results;
      state.syncedTokens = action.payload.syncedTokens;
      state.selectedMint = action.payload.selectedMint;
    },
    setSelectedMint(state, action: PayloadAction<string | null>) {
      state.selectedMint = action.payload;
    },
  },
});

export const { clearSyncOutput, setSyncOutput, setSelectedMint } = syncTokenSlice.actions;

export default syncTokenSlice.reducer;
