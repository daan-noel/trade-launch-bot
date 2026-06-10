import { configureStore } from '@reduxjs/toolkit';
import swingDetectionReducer from './swingDetectionSlice';
import syncTokenReducer from './syncTokenSlice';
import { apiSlice } from './apiSlice';

export const store = configureStore({
  reducer: {
    swingDetection: swingDetectionReducer,
    syncToken: syncTokenReducer,
    [apiSlice.reducerPath]: apiSlice.reducer,
  },
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware({
      serializableCheck: {
        // Synced token payloads carry potentially large trade arrays. They are
        // plain JSON (serializable), but deep-scanning them on every dispatch is
        // wasteful in dev, so skip the check for this slice's data.
        ignoredActions: ['syncToken/mergeSyncOutput'],
        ignoredPaths: ['syncToken.syncedTokens'],
      },
    }).concat(apiSlice.middleware),
});

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
