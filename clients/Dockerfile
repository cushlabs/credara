# syntax=docker/dockerfile:1
#
# Creda clients image. Multi-stage:
#   1. node:20 (pnpm) — install deps + tsc + vite build
#   2. nginx:alpine — serve dist/ with SPA history fallback
#
# Build context = repo root (matches testbed/images/*.Dockerfile pattern):
#   docker build -f clients/Dockerfile -t creda-clients:testbed .

# nginxinc/nginx-unprivileged is the upstream's purpose-built non-root variant:
#   - runs as uid 101 by default (matches the chart's runAsUser)
#   - listens on :8080 instead of :80 so it doesn't need CAP_NET_BIND_SERVICE
#   - writes the pid file under /tmp instead of /var/run/, so a read-only or
#     restricted-PSS-locked /var/run/ doesn't break startup
# Plain `nginx:alpine` *can* run non-root but only after extra fiddling with file ownership
# and pid paths; the unprivileged variant is the documented way to do this.
ARG NODE_BUILDER=docker.io/library/node:20-alpine
ARG RUNTIME=docker.io/nginxinc/nginx-unprivileged:1.27-alpine

FROM ${NODE_BUILDER} AS builder
RUN apk add --no-cache git
RUN corepack enable && corepack prepare pnpm@9.7.0 --activate
WORKDIR /src
COPY clients/package.json clients/pnpm-lock.yaml* ./
# `--ignore-scripts` keeps the install fast; playwright browsers aren't needed in the
# production image (they ship as part of the e2e container).
RUN pnpm install --frozen-lockfile --ignore-scripts || pnpm install --ignore-scripts
COPY clients/ ./
# Default to mock mode unless --build-arg VITE_FHIR_BASE=... is passed.
ARG VITE_FHIR_BASE=mock
ENV VITE_FHIR_BASE=${VITE_FHIR_BASE}
RUN pnpm build

FROM ${RUNTIME}
# nginx-unprivileged is configured for non-root use out of the box. We just drop our SPA
# config into /etc/nginx/conf.d/ and the static bundle into /usr/share/nginx/html. The base
# image already runs as uid 101 and listens on :8080 — no USER directive needed here.
COPY clients/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /src/dist /usr/share/nginx/html
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=2s --start-period=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1:8080/healthz || exit 1
CMD ["nginx", "-g", "daemon off;"]
