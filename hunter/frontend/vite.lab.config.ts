import { makeConfig } from './vite.config.base';

// LAB build (workstation only, never shipped). Served from `lab.html`; the
// `spaFallback` middleware rewrites deep client routes to `lab.html` so a hard
// refresh on e.g. `/strategies/tpsl1` loads the lab app instead of the live one.
// `/api` proxies to the lab bin, which binds :8140 by default (its `LAB_PORT`,
// matching LAB_API_PORT), off the live bin's :8130 — so `cargo run -p hunter-lab`
// needs no port override.
export default makeConfig({
  port: 5174,
  entry: 'lab.html',
  proxyTargetEnvKey: 'VITE_LAB_DEV_PROXY_TARGET',
  proxyTargetDefault: 'http://127.0.0.1:8140',
  spaFallback: true,
});
