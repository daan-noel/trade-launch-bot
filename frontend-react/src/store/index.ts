import { configureStore } from '@reduxjs/toolkit';
import walletReducer from './walletSlice';
import { apiSlice } from './apiSlice';

export const store = configureStore({
  reducer: {
    wallet: walletReducer,
    [apiSlice.reducerPath]: apiSlice.reducer,
  },
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware().concat(apiSlice.middleware),
});

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
