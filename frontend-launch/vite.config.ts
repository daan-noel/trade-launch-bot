import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5175,
    proxy: {
      '/api': {
        target: process.env.VITE_LIVE_PROXY ?? 'http://127.0.0.1:8091',
        changeOrigin: true,
      },
      '/health': {
        target: process.env.VITE_LIVE_PROXY ?? 'http://127.0.0.1:8091',
        changeOrigin: true,
      },
    },
  },
});
