import { configureStore } from '@reduxjs/toolkit';
import { baseApi } from '@shared/store/baseApi';
// Side-effect import: attaches all endpoints to baseApi before the store mounts.
import '@shared/store/endpoints';

export const store = configureStore({
  reducer: {
    [baseApi.reducerPath]: baseApi.reducer,
  },
  middleware: (getDefault) => getDefault().concat(baseApi.middleware),
});

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
