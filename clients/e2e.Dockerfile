# syntax=docker/dockerfile:1
#
# Playwright runner image — runs the e2e specs as a kubectl Job inside the kind cluster.
# Built separately from the clients image because Playwright browsers add ~400 MB and would
# bloat production. Uses the official Microsoft Playwright image (which bundles Chromium +
# the required Linux libraries).
#
# Build context = repo root:
#   docker build -f clients/e2e.Dockerfile -t creda-clients-e2e:testbed .

ARG PLAYWRIGHT=mcr.microsoft.com/playwright:v1.46.0-jammy

FROM ${PLAYWRIGHT}
RUN corepack enable && corepack prepare pnpm@9.7.0 --activate
WORKDIR /app

# Copy only what's needed to install + run tests. Source isn't built here; we run against
# the in-cluster clients Service (CLIENTS_URL).
COPY clients/package.json clients/pnpm-lock.yaml* ./
RUN pnpm install --frozen-lockfile --ignore-scripts || pnpm install --ignore-scripts

COPY clients/playwright.config.ts ./
COPY clients/tsconfig.json ./
COPY clients/e2e ./e2e

# The Job sets CLIENTS_URL=http://<service>:8080 so the config skips webServer and points
# Playwright straight at the cluster.
USER pwuser
ENTRYPOINT ["pnpm", "test:e2e"]
