import { createSlice, createAsyncThunk } from '@reduxjs/toolkit';
import { fetchWalletHoldings } from 'services/api';
import type { WalletHolding } from 'types';

interface WalletState {
  holdings: WalletHolding[];
  loading: boolean;
  error: string | null;
}

const initialState: WalletState = {
  holdings: [],
  loading: false,
  error: null,
};

export const loadWalletHoldings = createAsyncThunk('wallet/loadHoldings', async () => {
  return fetchWalletHoldings();
});

const walletSlice = createSlice({
  name: 'wallet',
  initialState,
  reducers: {},
  extraReducers: (builder) => {
    builder
      .addCase(loadWalletHoldings.pending, (state) => {
        state.loading = true;
        state.error = null;
      })
      .addCase(loadWalletHoldings.fulfilled, (state, action) => {
        state.loading = false;
        state.holdings = action.payload;
      })
      .addCase(loadWalletHoldings.rejected, (state, action) => {
        state.loading = false;
        state.error = action.error.message ?? 'Failed to load holdings';
      });
  },
});

export default walletSlice.reducer;
