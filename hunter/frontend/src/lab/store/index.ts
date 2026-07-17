import { configureStore } from '@reduxjs/toolkit';
import { setupListeners } from '@reduxjs/toolkit/query';
import { baseApi } from 'store/baseApi';
// Attach the shared + lab endpoints onto `baseApi` for side-effect BEFORE
// the store reads `baseApi.reducer`/`.endpoints`. Live endpoints are never
// imported here, so their `injectEndpoints` side effect (and code) stays out of
// the lab bundle.
import 'store/sharedEndpoints';
import './labEndpoints';

export const store = configureStore({
  reducer: {
    [baseApi.reducerPath]: baseApi.reducer,
  },
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware().concat(baseApi.middleware),
});

setupListeners(store.dispatch);

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
