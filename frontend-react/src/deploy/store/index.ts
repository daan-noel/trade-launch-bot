import { configureStore } from '@reduxjs/toolkit';
import { setupListeners } from '@reduxjs/toolkit/query';
import { baseApi } from 'store/baseApi';
import syncTokenReducer from '@deploy/slices/syncTokenSlice';
// Attach the shared + deploy endpoints onto `baseApi` for side-effect BEFORE the
// store reads `baseApi.reducer`/`.endpoints`. Analysis endpoints are never
// imported here, so their `injectEndpoints` side effect (and code) stays out of
// the deploy bundle.
import 'store/sharedEndpoints';
import './deployEndpoints';

export const store = configureStore({
  reducer: {
    syncToken: syncTokenReducer,
    [baseApi.reducerPath]: baseApi.reducer,
  },
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware({
      serializableCheck: {
        // Synced token payloads carry potentially large trade arrays — plain
        // JSON but wasteful to deep-scan on every dispatch, so skip this slice.
        ignoredActions: ['syncToken/mergeSyncOutput'],
        ignoredPaths: ['syncToken.syncedTokens'],
      },
    }).concat(baseApi.middleware),
});

setupListeners(store.dispatch);

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
