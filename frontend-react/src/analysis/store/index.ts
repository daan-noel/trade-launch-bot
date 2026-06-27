import { configureStore } from '@reduxjs/toolkit';
import { setupListeners } from '@reduxjs/toolkit/query';
import { baseApi } from 'store/baseApi';
import swingDetectionReducer from '@analysis/slices/swingDetectionSlice';
// Attach the shared + analysis endpoints onto `baseApi` for side-effect BEFORE
// the store reads `baseApi.reducer`/`.endpoints`. Deploy endpoints are never
// imported here, so their `injectEndpoints` side effect (and code) stays out of
// the analysis bundle.
import 'store/sharedEndpoints';
import './analysisEndpoints';

export const store = configureStore({
  reducer: {
    swingDetection: swingDetectionReducer,
    [baseApi.reducerPath]: baseApi.reducer,
  },
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware().concat(baseApi.middleware),
});

setupListeners(store.dispatch);

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
