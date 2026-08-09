import { defineConfig, loadEnv, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import tsconfigPaths from 'vite-tsconfig-paths';

// Single source of truth: the monorepo root .env (one level up from this package).
// Only VITE_*-prefixed vars are exposed to the client bundle; backend secrets in
// the same file stay server-side. NEVER give a secret a VITE_ prefix.
// (In Docker the build uses ENV build-args instead — see deploy/hunter-live/ui.Dockerfile.)
const ENV_DIR = '..';

export interface AppConfigOptions {
  /** Dev-server port (live = 5173, lab = 5174). */
  port: number;
  /** HTML entry for this build (`index.html` → live, `lab.html` → lab). */
  entry: string;
  /** `.env` key holding the `/api` dev-proxy target for this app. */
  proxyTargetEnvKey: string;
  /** Fallback proxy target when the env key is unset. */
  proxyTargetDefault: string;
  /**
   * When the entry is NOT `index.html`, Vite's SPA history fallback would serve
   * `index.html` — i.e. the *other* (live) app — for a deep client route. This
   * middleware rewrites top-level HTML navigations to this app's entry so a hard
   * refresh on e.g. `/strategies/tpsl1` loads the lab app, not the live one.
   */
  spaFallback?: boolean;
}

/** Rewrite top-level HTML navigations to `entry`, leaving assets/modules/@vite/api alone. */
const htmlFallback = (entry: string): Plugin => ({
  name: 'spa-html-fallback',
  configureServer(server) {
    server.middlewares.use((req, _res, next) => {
      // `connect`'s request type only exposes the node http fields when
      // `@types/node` is installed — this package intentionally omits it (the
      // app typechecks against DOM, not node). Read the few fields we need
      // through a narrow local type so the config compiles without node types.
      const { url = '', method = 'GET', headers } = req as {
        url?: string;
        method?: string;
        headers: { accept?: string };
      };
      if (
        method === 'GET' &&
        (headers.accept ?? '').includes('text/html') &&
        !url.startsWith('/@') &&
        !url.startsWith('/api') &&
        // Skip anything that looks like a file request (has an extension), incl.
        // `/lab.html` itself and `/src/lab/main.tsx`.
        !url.split('?')[0].includes('.')
      ) {
        (req as { url?: string }).url = `/${entry}`;
      }
      next();
    });
  },
});

/** Build a per-app Vite config so live and lab each run as their own dev server. */
export function makeConfig(opts: AppConfigOptions) {
  return defineConfig(({ mode }) => {
    const env = loadEnv(mode, ENV_DIR, '');
    const proxyTarget = env[opts.proxyTargetEnvKey] || opts.proxyTargetDefault;
    return {
      envDir: ENV_DIR,
      plugins: [
        react(),
        tailwindcss(),
        tsconfigPaths(),
        ...(opts.spaFallback ? [htmlFallback(opts.entry)] : []),
      ],
      // Single entry per app — the other app's HTML/code never enters this build.
      build: {
        rollupOptions: { input: { main: opts.entry } },
      },
      server: {
        port: opts.port,
        proxy: {
          '/api': {
            target: proxyTarget,
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
}
