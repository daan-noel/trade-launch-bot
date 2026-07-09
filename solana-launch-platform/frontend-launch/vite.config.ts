import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import tsconfigPaths from 'vite-tsconfig-paths';

export default defineConfig({
  plugins: [react(), tailwindcss(), tsconfigPaths()],
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
