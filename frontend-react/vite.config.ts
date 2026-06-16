import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import tsconfigPaths from 'vite-tsconfig-paths';

// Single source of truth: the monorepo root .env (one level up from this package).
// Only VITE_*-prefixed vars are exposed to the client bundle; backend secrets in
// the same file stay server-side. NEVER give a secret a VITE_ prefix.
// (In Docker the build uses ENV build-args instead — see frontend-react/Dockerfile.)
const ENV_DIR = '..';

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, ENV_DIR, '');
  return {
    envDir: ENV_DIR,
    plugins: [react(), tailwindcss(), tsconfigPaths()],
    server: {
      port: Number(env.VITE_DEV_PORT) || 5173,
      proxy: {
        '/api': {
          target: env.VITE_DEV_PROXY_TARGET || 'http://127.0.0.1:8081',
          changeOrigin: true,
          // Backend auth is fail-closed: mutating routes require a bearer token.
          // Inject it on the dev proxy (runs in Node, server-side) so the token
          // never reaches the browser bundle. In prod, nginx does the same.
          headers: env.API_AUTH_TOKEN
            ? { Authorization: `Bearer ${env.API_AUTH_TOKEN}` }
            : undefined,
        },
      },
    },
  };
});
