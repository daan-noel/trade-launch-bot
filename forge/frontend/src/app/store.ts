import { configureStore } from '@reduxjs/toolkit';
import { setupListeners } from '@reduxjs/toolkit/query';
import { baseApi } from '@shared/store/baseApi';
// Side-effect import: attaches all endpoints to baseApi before the store mounts.
import '@shared/store/endpoints';

export const store = configureStore({
  reducer: {
    [baseApi.reducerPath]: baseApi.reducer,
  },
  middleware: (getDefault) => getDefault().concat(baseApi.middleware),
});

// Wire RTK Query's focus/reconnect listeners onto the store dispatch. Without
// this, `refetchOnFocus` / `refetchOnReconnect` / `skipPollingIfUnfocused` on
// individual queries are all inert — backgrounded tabs would poll forever.
setupListeners(store.dispatch);

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
