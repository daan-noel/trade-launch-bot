# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# hunter-lab UI image — builds the LAB React app (`npm run build:lab`, entry
# `lab.html`) to static files, then serves them with nginx and reverse-proxies
# /api to the lab backend container.
#
# The lab stack deploys to its OWN server (not the live EC2 box — the analysis
# bin is too heavy for it), so this UI mirrors the live UI's setup: TLS + Basic
# auth + bearer injection. The live UI is a SEPARATE image
# (deploy/hunter-live/ui.Dockerfile) built from `npm run build:live` (entry
# `index.html`); the two builds never share a bundle — see
# hunter/frontend/vite.config.base.ts (single entry per app).
#
# Build context = repo root (so paths carry the hunter/ prefix). See compose.yml.
# ---------------------------------------------------------------------------

FROM node:22-bookworm-slim AS build
WORKDIR /app

# Install deps first for layer caching. Paths are relative to the monorepo root
# (the build context) — hence the hunter/ prefix.
COPY hunter/frontend/package*.json ./
# package-lock.json is written by npm 11 (local toolchain), but node:22-slim
# ships npm 10, whose stricter `npm ci` sync-check rejects it. Match the major
# so `npm ci` stays deterministic.
RUN npm install -g npm@11 && npm ci

# Build-time config baked into the static bundle.
# Empty API base => frontend uses relative /api (proxied by nginx, same-origin).
ARG VITE_API_BASE=""
ARG VITE_POLL_INTERVAL_MS=5000
ENV VITE_API_BASE=$VITE_API_BASE
ENV VITE_POLL_INTERVAL_MS=$VITE_POLL_INTERVAL_MS

COPY hunter/frontend/ ./
# build:lab => `tsc && vite build --config vite.lab.config.ts` (entry lab.html).
# Vite preserves the entry basename, so the output document is dist/lab.html.
RUN npm run build:lab

# --- Serve -----------------------------------------------------------------
FROM nginx:1.27-alpine AS runtime
# Config is a template: the nginx image entrypoint runs envsubst over
# /etc/nginx/templates/*.template at startup, writing the result to
# /etc/nginx/conf.d/default.conf. This injects ${API_AUTH_TOKEN} (passed as an
# env var by compose) into the /api proxy's bearer header without baking the
# secret into the image. NGINX_ENVSUBST_FILTER (set in compose) limits
# substitution to API_AUTH_TOKEN so nginx's own $-variables survive.
COPY deploy/hunter-lab/nginx/default.conf.template /etc/nginx/templates/default.conf.template
COPY deploy/hunter-lab/nginx/.htpasswd /etc/nginx/.htpasswd
# Self-signed TLS cert+key for local/testing. The .pem files are gitignored —
# you must generate them before building (see nginx/tls/README.md).
COPY deploy/hunter-lab/nginx/tls/cert.pem deploy/hunter-lab/nginx/tls/key.pem /etc/nginx/tls/
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80 443
