# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# forge-live UI image — builds the forge React/Vite frontend to static files,
# then serves them with nginx and reverse-proxies /api to the live backend
# container. The SPA calls /api/... same-origin (baseApi.ts uses an empty base
# URL), so there is no CORS and no hardcoded backend URL.
#
# Unlike hunter, the forge backend has NO fail-closed bearer gate, so the nginx
# config is a PLAIN default.conf (TLS + Basic-auth only, no ${API_AUTH_TOKEN}
# injection / envsubst) — see nginx/default.conf.
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
# Plain conf copied straight into conf.d (NOT templates/) — nginx only runs
# envsubst over /etc/nginx/templates/*, and forge has no template variable to
# substitute, so this keeps nginx's own $host/$remote_addr runtime vars intact.
COPY deploy/forge-live/nginx/default.conf /etc/nginx/conf.d/default.conf
COPY deploy/forge-live/nginx/.htpasswd /etc/nginx/.htpasswd
# Self-signed TLS cert+key for local/testing. The .pem files are gitignored —
# you must generate them before building (see nginx/tls/README.md).
COPY deploy/forge-live/nginx/tls/cert.pem deploy/forge-live/nginx/tls/key.pem /etc/nginx/tls/
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80 443
