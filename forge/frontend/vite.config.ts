import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import tsconfigPaths from 'vite-tsconfig-paths';

// Load env from the forge product root (one level up from this package) so the
// dev proxy can read the backend `API_AUTH_TOKEN`. Only VITE_*-prefixed vars are
// exposed to the client bundle; backend secrets stay server-side. NEVER give a
// secret a VITE_ prefix.
const ENV_DIR = '..';

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, ENV_DIR, '');
  const target = env.VITE_LIVE_PROXY || 'http://127.0.0.1:8230';
  // Backend auth is fail-closed: mutating routes require a bearer token. Inject
  // it on the dev proxy (runs in Node, server-side) so the token never reaches
  // the browser bundle. In prod, nginx does the same. Absent token ⇒ omit the
  // header (the backend refuses to boot without one, so this only bites in dev).
  const authHeaders = env.API_AUTH_TOKEN
    ? { Authorization: `Bearer ${env.API_AUTH_TOKEN}` }
    : undefined;
  return {
    envDir: ENV_DIR,
    plugins: [react(), tailwindcss(), tsconfigPaths()],
    server: {
      port: 5175,
      proxy: {
        '/api': {
          target,
          changeOrigin: true,
          headers: authHeaders,
        },
        '/health': {
          target,
          changeOrigin: true,
        },
      },
    },
  };
});
