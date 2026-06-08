#!/bin/sh
# Container entrypoint for the clients image. Two deploy-time switches:
#
#   FHIR_BASE   — written into /tmp/runtime-config.js as window.__CREDA_FHIR_BASE__.
#                 Empty / "mock" → SPA uses the in-memory mock adapter.
#                 "/fhir"        → SPA fetches relative /fhir/*; nginx (below) proxies that
#                                  to BRIDGE_UPSTREAM. No CORS, single origin.
#                 http(s) URL    → SPA fetches the URL directly; bridge must allow CORS.
#
#   BRIDGE_UPSTREAM — only consulted when FHIR_BASE=/fhir. The upstream nginx proxies to,
#                     e.g. http://creda-bridge:8080. Empty disables the proxy block.
#
# ## Why this writes to /tmp and not the image filesystem
#
# /usr/share/nginx/html (the SPA bundle) and /etc/nginx/conf.d (the static config) are
# image-layer paths. Under non-root images + rootless podman / restricted PSS, writes to
# either of those layers fail with "Permission denied" even when the build-time chown looked
# correct (rootless podman uid-maps don't always propagate through every layer cleanly).
# Writing to /tmp sidesteps the whole question: /tmp is a writable emptyDir / tmpfs in
# every k8s pod and writable by any uid in any sane container runtime.
#
# The static nginx default.conf (in /etc/nginx/conf.d) is COPYed at build time and never
# modified at runtime — it serves /runtime-config.js by alias-pointing at /tmp/runtime-config.js,
# and it includes /tmp/fhir-overrides.d/*.conf so the /fhir proxy can be added conditionally.

set -eu

FHIR_BASE="${FHIR_BASE:-}"
BRIDGE_UPSTREAM="${BRIDGE_UPSTREAM:-}"
RUNTIME_CFG="/tmp/runtime-config.js"
PROXY_DROPIN_DIR="/tmp/fhir-overrides.d"

mkdir -p "$PROXY_DROPIN_DIR"
# Clear any stale overrides from a previous container generation (defensive — /tmp would
# usually be empty on pod start, but a `kubectl exec`-style reuse could leave files behind).
rm -f "$PROXY_DROPIN_DIR"/*.conf 2>/dev/null || true

# ---- 1. SPA runtime config (always emitted) ----------------------------------------------
cat > "$RUNTIME_CFG" <<EOF
// Generated at container start by /docker-entrypoint.d/40-creda.sh.
// Overrides the inline default in index.html. See FHIR_BASE in the entrypoint script.
window.__CREDA_FHIR_BASE__ = "${FHIR_BASE}";
EOF
echo "==> SPA runtime: window.__CREDA_FHIR_BASE__ = \"${FHIR_BASE}\" (via $RUNTIME_CFG)"

# ---- 2. optional nginx /fhir reverse-proxy drop-in ---------------------------------------
if [ "$FHIR_BASE" = "/fhir" ] && [ -n "$BRIDGE_UPSTREAM" ]; then
  cat > "$PROXY_DROPIN_DIR/fhir.conf" <<EOF
# Generated at container start (BRIDGE_UPSTREAM=${BRIDGE_UPSTREAM}). Proxies /fhir/* to the
# bridge so the SPA can talk to it from the same origin (no CORS).
location /fhir/ {
  proxy_pass ${BRIDGE_UPSTREAM}/fhir/;
  proxy_http_version 1.1;
  proxy_set_header Host \$host;
  proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
  proxy_set_header X-Forwarded-Proto \$scheme;
  proxy_read_timeout 30s;
}
EOF
  echo "==> nginx /fhir/ → ${BRIDGE_UPSTREAM}/fhir/ (via $PROXY_DROPIN_DIR/fhir.conf)"
else
  echo "==> nginx /fhir/ proxy disabled (FHIR_BASE='${FHIR_BASE}', BRIDGE_UPSTREAM='${BRIDGE_UPSTREAM}')"
fi

# Do NOT exec nginx here — the nginx-unprivileged base image's main entrypoint walks
# /docker-entrypoint.d/*.sh in lex order and then exec's nginx itself via its CMD.
