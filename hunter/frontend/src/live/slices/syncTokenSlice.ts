import { createSlice, type PayloadAction } from '@reduxjs/toolkit';
import type { TokenDetailRecord, TradeRecord } from 'types';

/** A single token's sync outcome. */
export type SyncResultItem = { mint_address: string; ok: boolean; error?: string };

/** A token that synced successfully, paired with its trades. */
export type SyncedToken = { token: TokenDetailRecord; trades: TradeRecord[] };

/**
 * The numeric result of a sync preview, cached for reuse. Counting signatures
 * hits Helius (slow/costly), so results are kept keyed by `${mint}|${flag}` and
 * reused until a sync of that mint invalidates them.
 */
export type SyncPreviewData = {
  newCount: number;
  newCapped: boolean;
  totalCount: number;
  totalCapped: boolean;
};

/**
 * Upper bound on the heavy per-token output (results + their token/trade
 * records) retained in Redux. Sync runs over large mint lists would otherwise
 * grow these arrays — and the O(n) merge scan over them — without limit for the
 * whole session. We keep the most-recent {@link MAX_RETAINED_TOKENS} in
 * first-seen order and evict the oldest, dropping their cached previews too.
 */
const MAX_RETAINED_TOKENS = 200;

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
  /**
   * Cached "to fetch" estimates from {@link SyncPreviewData}, keyed by
   * `${mint}|${includePostMigrate}`. Persisted here so the input status panel
   * survives navigation and skips re-counting mints it has already priced.
   */
  syncPreviews: Record<string, SyncPreviewData>;
}

const initialState: SyncTokenState = {
  results: [],
  syncedTokens: [],
  selectedMint: null,
  syncedMints: [],
  syncPreviews: {},
};

const syncTokenSlice = createSlice({
  name: 'syncToken',
  initialState,
  reducers: {
    /** Reset all accumulated sync output, the mint cache, and cached previews. */
    clearSyncOutput(state) {
      state.results = [];
      state.syncedTokens = [];
      state.selectedMint = null;
      state.syncedMints = [];
      // Drop cached previews too: with the output gone the user is starting
      // fresh, so any re-added mint should be re-counted rather than read a
      // stale estimate. (Previously these leaked for the whole session.)
      state.syncPreviews = {};
    },
    /** Store a freshly fetched sync preview under its `${mint}|${flag}` key. */
    cacheSyncPreview(
      state,
      action: PayloadAction<{ key: string; data: SyncPreviewData }>,
    ) {
      state.syncPreviews[action.payload.key] = action.payload.data;
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
        const idx = state.results.findIndex((x) => x.mint_address === r.mint_address);
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
        if (!state.syncedMints.includes(r.mint_address)) state.syncedMints.push(r.mint_address);
      }
      // Cap the heavy output so a long session over many mints can't grow these
      // arrays (and the merge scan above) unbounded. Evict the oldest results
      // and drop their token/trade records + cached previews; `syncedMints`
      // stays (cheap string list) so the user can still reload the full set.
      if (state.results.length > MAX_RETAINED_TOKENS) {
        const evicted = state.results.splice(
          0,
          state.results.length - MAX_RETAINED_TOKENS,
        );
        const dropped = new Set(evicted.map((r) => r.mint_address));
        state.syncedTokens = state.syncedTokens.filter(
          (t) => !dropped.has(t.token.mint_address),
        );
        for (const mint of dropped) {
          delete state.syncPreviews[`${mint}|true`];
          delete state.syncPreviews[`${mint}|false`];
        }
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

export const {
  clearSyncOutput,
  mergeSyncOutput,
  setSelectedMint,
  cacheSyncPreview,
} = syncTokenSlice.actions;

export default syncTokenSlice.reducer;
