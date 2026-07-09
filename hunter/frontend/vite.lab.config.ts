import { makeConfig } from './vite.config.base';

// LAB build (workstation only, never shipped). Served from `lab.html`; the
// `spaFallback` middleware rewrites deep client routes to `lab.html` so a hard
// refresh on e.g. `/strategies/tpsl1` loads the lab app instead of the live one.
// `/api` proxies to the lab bin (default :8082 — run `PORT=8082 cargo run -p lab`
// to keep it off the live bin's :8081 when both run side by side).
export default makeConfig({
  port: 5174,
  entry: 'lab.html',
  proxyTargetEnvKey: 'VITE_LAB_DEV_PROXY_TARGET',
  proxyTargetDefault: 'http://127.0.0.1:8082',
  spaFallback: true,
});
