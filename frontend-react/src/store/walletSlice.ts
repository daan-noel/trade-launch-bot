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

export const loadWalletHoldings = createAsyncThunk(
  'wallet/loadHoldings',
  async () => {
    return fetchWalletHoldings();
  },
  {
    // Skip if a load is already in flight. Without this, React 18 StrictMode's
    // double-mounted effect fires two real Solana-RPC reads on every page open.
    // The retry-loop in MyWalletPage still works: each attempt awaits the prior
    // dispatch to settle (loading back to false) before the next one runs.
    condition: (_arg, { getState }) => {
      const { wallet } = getState() as { wallet: WalletState };
      return !wallet.loading;
    },
  },
);

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
