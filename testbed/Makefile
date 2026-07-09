# Creda testbed — top-level targets.
#
# Bring up a kind cluster, build & load the Core/Bridge images, run scenarios, tear down.
#
# All script invocations go through `bash <path>` rather than executing the script directly, so
# the testbed does not depend on the executable bit being set in the developer's clone. This
# matters because git only preserves the executable bit when it was set in the commit that
# introduced the file — a tester pulling a fresh clone of a new scenario otherwise hits a
# permission-denied error before the test even starts.

SHELL := /usr/bin/env bash
.SHELLFLAGS := -eu -o pipefail -c

CLUSTER_NAME ?= creda-testbed
KIND         ?= kind
KUBECTL      ?= kubectl
HELM         ?= helm
BASH         ?= bash
# Container engine for image reclaim. `docker` resolves to podman on the maintainers' macs (the
# same way build-and-load.sh shells out to `docker build`); override with ENGINE=podman if needed.
ENGINE       ?= docker

REPO_ROOT := $(realpath $(dir $(lastword $(MAKEFILE_LIST)))/..)

.PHONY: up down clean reclaim images smoke ae-repair revocation-latency partition-rejoin rogue-link ui-smoke ui-up ui-forward ui-down ui-up-real ui-down-real reset seed

## --- top-level lifecycle ---
## Docker is the only host prerequisite. The peer-driver, like Core and Bridge, is built inside
## the dev container and shipped as an image; it runs in-cluster as a Kubernetes Job. No host
## Rust toolchain required.

## `up` is a *clean* bring-up: tear down any existing cluster, reclaim dangling images + build
## cache, then recreate and load. This keeps repeated triage iterations from stacking stale
## clusters/containers and accreting image layers (the "hundreds of processes" problem). Prereqs
## run in listed order (the testbed never runs make with -j). To bring up WITHOUT the teardown,
## run `make cluster images` directly.
up: down reclaim cluster images
	@echo "==> testbed cluster up (clean). next: make smoke"

cluster:
	$(BASH) $(REPO_ROOT)/testbed/kind/up.sh "$(CLUSTER_NAME)"

images:
	$(BASH) $(REPO_ROOT)/testbed/images/build-and-load.sh "$(CLUSTER_NAME)"

down:
	$(BASH) $(REPO_ROOT)/testbed/kind/down.sh "$(CLUSTER_NAME)"

## Reclaim ONLY dangling (untagged) images and stale build cache — never tagged/in-use images
## like creda-*:testbed or creda-dev:local, so the next build still hits its cache. This is the
## accretion left by repeated rebuilds (each `:testbed` rebuild orphans the previous layer). The
## leading `-` keeps a prune failure (e.g. engine not running) from aborting `up`.
reclaim:
	@echo "==> reclaiming dangling images + build cache ($(ENGINE))"
	-$(ENGINE) image prune -f
	-$(ENGINE) builder prune -f

clean: down
	@rm -rf $(REPO_ROOT)/testbed/.run
	@echo "==> testbed cleaned"

## --- scenarios ---

smoke:
	$(BASH) $(REPO_ROOT)/testbed/scenarios/gossip-convergence/run.sh "$(CLUSTER_NAME)"

# Anti-entropy repair: verifies §6.1.8 (AE backstop for gossip). Late-joining peer catches up
# via periodic manifest exchange + delta fetch. Slower than smoke (75s budget) — AE_INTERVAL_SECS
# is 30s today and the first useful round is at ~t=30s after the joining peer's daemon starts.
ae-repair:
	$(BASH) $(REPO_ROOT)/testbed/scenarios/anti-entropy-repair/run.sh "$(CLUSTER_NAME)"

# Revocation latency: verifies §4.7 revocation propagation over real gossip. A Grant + Revocation
# are injected at peer-a; the Grant is confirmed replicated at peer-b first (so the Revocation is
# validated on arrival, §4.6 step 2), then the Revocation's propagation to peer-b is measured
# against Bound 1. Adds new peer-driver subcommands — run `make up`/`make images` first.
revocation-latency:
	$(BASH) $(REPO_ROOT)/testbed/scenarios/revocation-latency/run.sh "$(CLUSTER_NAME)"

# Partition-rejoin: verifies §6.1.7 partition tolerance + §6.1.8 anti-entropy backstop. A real
# node-level iptables partition between the two peers; both keep writing; isolation is asserted;
# then heal and the DAGs reconcile via anti-entropy. Adds the check-absent peer-driver subcommand —
# run `make up`/`make images` first. Slower (~2-3 min; reconciliation is AE-paced).
partition-rejoin:
	$(BASH) $(REPO_ROOT)/testbed/scenarios/partition-rejoin/run.sh "$(CLUSTER_NAME)"

# Rogue-link: verifies the §4.6 step 5.5 cross-institutional link-chain defense over real gossip. A
# rogue peer fuses a self-issued Grant onto the responder's patient via a manual Link (denied) and
# via an insurance-crosswalk Link (admitted); the deny-by-default responder's EvaluateAuthorization
# must reject the former and honor the latter. Adds the inject-link/check-authz peer-driver
# subcommands and the Core link-chain wiring — run `make up`/`make images` first.
rogue-link:
	$(BASH) $(REPO_ROOT)/testbed/scenarios/rogue-link/run.sh "$(CLUSTER_NAME)"

# UI smoke: deploys the persona front-end clients (clients/) into the cluster and runs the
# Playwright e2e specs as an in-cluster Job. Mock-mode (no bridge wiring) for now; once the
# M7 bridge stubs land the same scenario will run against a real bridge.
ui-smoke:
	$(BASH) $(REPO_ROOT)/testbed/scenarios/ui-smoke/run.sh "$(CLUSTER_NAME)"

## --- UI for user-acceptance testing ---
##
## ui-up brings the persona clients up in their own persistent namespace ('creda-ui') so a
## reviewer can drive the UI in a browser. It is intentionally separate from ui-smoke (which
## tears its namespace down at the end) — a test run never disturbs an in-progress UAT
## session. ui-forward is the blocking port-forward; ui-down tears the UAT namespace down.
ui-up:
	$(BASH) $(REPO_ROOT)/testbed/scripts/ui-up.sh "$(CLUSTER_NAME)"

ui-forward:
	$(BASH) $(REPO_ROOT)/testbed/scripts/ui-forward.sh "$(CLUSTER_NAME)"

ui-down:
	$(BASH) $(REPO_ROOT)/testbed/scripts/ui-down.sh "$(CLUSTER_NAME)"

## --- Real-mode UAT ---
##
## ui-up-real brings up a real Creda peer (Core + Bridge) alongside the clients, points the
## clients' nginx /fhir proxy at the bridge, and leaves it all running. Clicking 'Attest' in
## the clinician UI then produces a signed event in Core and publishes it to the gossip mesh
## for real. After it returns: `UAT=1 make ui-forward` in a second terminal to open the UI at
## http://localhost:5173. Distinct from ui-up (mock mode) — the two namespaces coexist.
ui-up-real:
	$(BASH) $(REPO_ROOT)/testbed/scripts/ui-up-real.sh "$(CLUSTER_NAME)"

ui-down-real:
	$(BASH) $(REPO_ROOT)/testbed/scripts/ui-down-real.sh "$(CLUSTER_NAME)"

## --- UAT data lifecycle ---
##
## The DAG is append-forward — events cannot be deleted — so returning the UAT peer to a known
## base state means wiping its event store and reseeding, NOT cycling the cluster or images.
## `reset` does exactly that (~60-90s): scale peer down, drop the data PVC, scale up, seed the
## demo dataset (Maria Gonzalez + James Whitfield, stable tok:demo:* lookup tokens). `seed`
## seeds WITHOUT wiping — note repeated seeds append duplicate demo events; prefer `reset`.
reset:
	$(BASH) $(REPO_ROOT)/testbed/scripts/reset-uat.sh "$(CLUSTER_NAME)"

seed:
	$(BASH) $(REPO_ROOT)/testbed/scripts/seed-demo.sh "$(CLUSTER_NAME)"
