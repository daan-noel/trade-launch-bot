# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Builds the React/Vite frontend to static files, then serves them with nginx.
# nginx also reverse-proxies /api to the backend container (see nginx/default.conf),
# so the SPA calls /api/... same-origin -> no CORS, no hardcoded backend URL.
#
# Build context = repo root (so we can also COPY the nginx config). See docker-compose.yml.
# ---------------------------------------------------------------------------

FROM node:22-bookworm-slim AS build
WORKDIR /app

# Install deps first for layer caching.
COPY frontend-react/package*.json ./
RUN npm ci

# Build-time config baked into the static bundle.
# Empty API base => frontend uses relative /api (proxied by nginx, same-origin).
ARG VITE_API_BASE=""
ARG VITE_POLL_INTERVAL_MS=5000
ENV VITE_API_BASE=$VITE_API_BASE
ENV VITE_POLL_INTERVAL_MS=$VITE_POLL_INTERVAL_MS

COPY frontend-react/ ./
RUN npm run build

# --- Serve -----------------------------------------------------------------
FROM nginx:1.27-alpine AS runtime
COPY nginx/default.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80
