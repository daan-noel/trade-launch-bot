# API Auth & Deploy Flow (fail-closed bearer)

## The problem this solves

The mutating API routes spend **real SOL** (`POST /api/solana/wallet/{buy,sell}`,
`/cashback/claim`, the `DELETE` routes). The auth middleware used to be
**fail-open**: it required a bearer token *only if* `API_AUTH_TOKEN` happened to
be set, and let everything through when it wasn't. A forgotten env var silently
exposed the money path.

The fix flips it to **fail-closed** and wires the token end-to-end so the
deployed stack still works without ever putting the secret in the browser.

## The two-layer defense

| Layer | Gate | Stops |
| --- | --- | --- |
| nginx Basic-Auth | username + password login (`.htpasswd`) | random internet traffic |
| backend bearer (fail-closed) | `Authorization: Bearer <API_AUTH_TOKEN>` | anything reaching the backend without nginx's injected token |

Only the **web** (nginx) containers are published. `postgres`, `live-api` and `lab-api` stay on
the internal compose network — unreachable from outside.

## Why the token can't live in the frontend

The React app is static JavaScript downloaded by the browser. **Anything baked
into it is public** — that's why `.env` forbids giving a secret a `VITE_` prefix.
So the SPA sends *no* token. Instead, whatever sits between the browser and the
backend (nginx in prod, the Vite dev proxy in dev) injects the bearer
**server-side**. The token never leaves the server.

---

## Code pieces (source of truth)

| Concern | Location |
| --- | --- |
| Fail-closed middleware | `live/src/main.rs` — `require_bearer_auth` |
| Token required at startup | `trading_core/src/config/settings.rs` — `required_non_empty("API_AUTH_TOKEN")` |
| Prod bearer injection | `nginx/default.conf.template` — `proxy_set_header Authorization "Bearer ${API_AUTH_TOKEN}"` |
| envsubst wiring | `frontend/Dockerfile` (template → `/etc/nginx/templates/`) + `docker-compose.yml` (`API_AUTH_TOKEN`, `NGINX_ENVSUBST_FILTER`) |
| Dev bearer injection | `frontend/vite.live.config.ts` / `vite.lab.config.ts` — dev proxy `headers` |

### The middleware rule (`require_bearer_auth`)

```
GET / HEAD / OPTIONS            → always allowed (safe reads + CORS preflight)
POST/PUT/DELETE/PATCH + token   → header must equal API_AUTH_TOKEN, else 401
POST/PUT/DELETE/PATCH + NO token → 401  (fail closed)
```

The last line is the change. Combined with `Settings::from_env` rejecting a
missing/empty `API_AUTH_TOKEN`, the server **refuses to boot** without a token —
so the fail-closed `None` arm is really only reachable in theory.

---

## Deploy: step by step (`docker compose up -d --build`)

1. **Compose reads `.env`.** Pulls in `API_AUTH_TOKEN`. The `:?` guard in
   `docker-compose.yml` aborts the deploy with an error if it's missing — no
   accidental fail-open.
2. **postgres starts**, runs its healthcheck. Backend waits for healthy.
3. **backend starts.** `Settings::from_env()` requires `API_AUTH_TOKEN`
   (non-empty) or the process exits. With it, ingest + strategies + HTTP server
   (`:8130`, internal only) come up. Mutating requests now need the bearer.
4. **web (nginx) starts.** The nginx image entrypoint runs **envsubst** over
   `default.conf.template`, replacing `${API_AUTH_TOKEN}` with the real value and
   writing the final `/etc/nginx/conf.d/default.conf`. `NGINX_ENVSUBST_FILTER=API_AUTH_TOKEN`
   restricts substitution to that one var so nginx's own `$host`/`$remote_addr`
   runtime variables survive untouched.

## Request flow (e.g. clicking "Buy")

```
   Browser                    nginx (web)                 backend
      │  open https://...         │                          │
      │ ────────────────────────> │                          │
      │  Basic-Auth login         │ (checks .htpasswd)        │
      │ <───── prompt ─────────── │                          │
      │  user/pass                │ ✅ login OK               │
      │ ────────────────────────> │                          │
      │                           │                          │
      │  POST /api/.../buy        │                          │
      │  (no token — browser      │                          │
      │   holds no secret)        │                          │
      │ ────────────────────────> │ swaps the header:        │
      │                           │ Authorization:           │
      │                           │ Bearer <API_AUTH_TOKEN>  │
      │                           │ ───────────────────────> │ bearer matches ✅
      │                           │                          │ → executes the buy
      │                           │ <─────── 200 OK ──────── │
      │ <──────── result ──────── │                          │
```

The browser authenticates to **nginx** with the login. nginx discards that
inbound `Authorization: Basic ...` and **overwrites** it with the bearer token on
the way to the backend. The backend trusts the request; the secret never reached
the browser.

## Dev mode (no nginx)

`npm run dev:live` + `cargo run -p live` (or `dev:lab` + `cargo run -p lab`).
There's no proxy container, so the **Vite dev proxy** does the injection:
`vite.live.config.ts` / `vite.lab.config.ts` reads `API_AUTH_TOKEN` from the
root `.env` via `loadEnv(mode, '..', '')` (non-`VITE_` vars stay server-side, in
the Node dev server — never in the bundle) and adds the `Authorization: Bearer`
header to every proxied `/api` call.

**Gotcha:** `API_AUTH_TOKEN` must be set in `.env`, or (a) the backend won't
start and (b) the dev proxy won't add the header → mutating calls would 401.

## Rotating the token

1. Replace `API_AUTH_TOKEN` in `.env` (generate, e.g. `head -c 32 /dev/urandom | base64`).
2. Prod: `docker compose up -d --build web backend` (nginx re-runs envsubst,
   backend reloads the new token).
3. Dev: restart `cargo run` and the Vite dev server.

No frontend rebuild is needed — the token is never compiled into the SPA.
