import { createSlice, type PayloadAction } from '@reduxjs/toolkit';
import type { SwingBatchEntry, SwingDetectionResult } from 'types';

function toDatetimeLocalValue(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function startOfToday(): Date {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d;
}

interface SwingDetectionState {
  /**
   * Whether the user has triggered the token fetch. The list itself lives in the
   * RTK Query cache; persisting this flag in Redux means navigating back to the
   * page re-displays the cached results instead of starting from a blank table.
   */
  tokensFetched: boolean;
  /** Token "created at" range filter (datetime-local strings, '' when cleared). */
  createdFrom: string;
  createdTo: string;
  /** Mint of the selected token row, or null. */
  selectedMint: string | null;
  /** Swing detection result for the selected token, or null. */
  swingResult: SwingDetectionResult | null;
  /** Key of the selected swing-leg row, or null. */
  selectedSwingKey: string | null;
  /**
   * "Swing Detection All" batch result — per-mint swing ledgers from the last
   * global run, or null if it hasn't run. Kept in Redux (as the serializable
   * array the API returns, not a Map) so the chain columns and per-token chart
   * overlay survive navigation away from the page.
   */
  swingAllResults: SwingBatchEntry[] | null;
}

const initialState: SwingDetectionState = {
  tokensFetched: false,
  createdFrom: toDatetimeLocalValue(startOfToday()),
  createdTo: toDatetimeLocalValue(new Date()),
  selectedMint: null,
  swingResult: null,
  selectedSwingKey: null,
  swingAllResults: null,
};

const swingDetectionSlice = createSlice({
  name: 'swingDetection',
  initialState,
  reducers: {
    setTokensFetched(state, action: PayloadAction<boolean>) {
      state.tokensFetched = action.payload;
    },
    setCreatedFrom(state, action: PayloadAction<string>) {
      state.createdFrom = action.payload;
    },
    setCreatedTo(state, action: PayloadAction<string>) {
      state.createdTo = action.payload;
    },
    clearCreatedRange(state) {
      state.createdFrom = '';
      state.createdTo = '';
    },
    /** Selecting a different token invalidates its swing result and leg selection. */
    setSelectedMint(state, action: PayloadAction<string | null>) {
      state.selectedMint = action.payload;
      state.swingResult = null;
      state.selectedSwingKey = null;
    },
    /** A fresh detection run (or clearing it) drops any previously selected leg. */
    setSwingResult(state, action: PayloadAction<SwingDetectionResult | null>) {
      state.swingResult = action.payload;
      state.selectedSwingKey = null;
    },
    setSelectedSwingKey(state, action: PayloadAction<string | null>) {
      state.selectedSwingKey = action.payload;
    },
    /** Store (or clear) the latest "Swing Detection All" batch result. */
    setSwingAllResults(state, action: PayloadAction<SwingBatchEntry[] | null>) {
      state.swingAllResults = action.payload;
    },
  },
});

export const {
  setTokensFetched,
  setCreatedFrom,
  setCreatedTo,
  clearCreatedRange,
  setSelectedMint,
  setSwingResult,
  setSelectedSwingKey,
  setSwingAllResults,
} = swingDetectionSlice.actions;

export default swingDetectionSlice.reducer;
