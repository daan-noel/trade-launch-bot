# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# forge-live UI image — builds the forge React/Vite frontend to static files,
# then serves them with nginx and reverse-proxies /api to the live backend
# container. The SPA calls /api/... same-origin (baseApi.ts uses an empty base
# URL), so there is no CORS and no hardcoded backend URL.
#
# Like hunter, the forge backend runs a fail-closed bearer gate (it moves
# treasury SOL), so the nginx config is a *.template: the image entrypoint runs
# envsubst over /etc/nginx/templates/*.template at startup, injecting
# ${API_AUTH_TOKEN} into the /api proxy's bearer header — see nginx/default.conf.template.
#
# Build context = repo root (so paths carry the forge/ prefix, and we can COPY
# the nginx config). See compose.yml.
# ---------------------------------------------------------------------------

FROM node:22-bookworm-slim AS build
WORKDIR /app

# Install deps first for layer caching. Paths are relative to the monorepo root
# (the build context) — hence the forge/ prefix.
COPY forge/frontend/package*.json ./
# package-lock.json is written by npm 11 (local toolchain), but node:22-slim
# ships npm 10, whose stricter `npm ci` sync-check rejects it. Match the major
# so `npm ci` stays deterministic.
RUN npm install -g npm@11 && npm ci

COPY forge/frontend/ ./
# `build` => `tsc && vite build` (single entry index.html).
RUN npm run build

# --- Serve -----------------------------------------------------------------
FROM nginx:1.27-alpine AS runtime
# Template copied into templates/ (NOT conf.d/): the nginx entrypoint runs
# envsubst over /etc/nginx/templates/*.template at startup and writes the result
# to /etc/nginx/conf.d/default.conf, injecting ${API_AUTH_TOKEN} (passed as an
# env var by compose). NGINX_ENVSUBST_FILTER (set in compose) limits substitution
# to that one var so nginx's own $host/$remote_addr runtime vars survive.
COPY deploy/forge-live/nginx/default.conf.template /etc/nginx/templates/default.conf.template
COPY deploy/forge-live/nginx/.htpasswd /etc/nginx/.htpasswd
# Self-signed TLS cert+key for local/testing. The .pem files are gitignored —
# you must generate them before building (see nginx/tls/README.md).
COPY deploy/forge-live/nginx/tls/cert.pem deploy/forge-live/nginx/tls/key.pem /etc/nginx/tls/
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80 443
