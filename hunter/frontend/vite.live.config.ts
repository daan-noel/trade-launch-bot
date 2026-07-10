import { makeConfig } from './vite.config.base';

// LIVE build (ships to EC2). Served at `/` from `index.html`; default Vite SPA
// fallback already serves `index.html` for deep routes, so no rewrite needed.
// `/api` proxies to the live bin (default :8130 — matches LIVE_PORT/LIVE_API_PORT).
export default makeConfig({
  port: 5173,
  entry: 'index.html',
  proxyTargetEnvKey: 'VITE_LIVE_DEV_PROXY_TARGET',
  proxyTargetDefault: 'http://127.0.0.1:8130',
});
