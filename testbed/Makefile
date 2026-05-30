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

REPO_ROOT := $(realpath $(dir $(lastword $(MAKEFILE_LIST)))/..)

.PHONY: up down clean images smoke ae-repair ui-smoke ui-up ui-forward ui-down

## --- top-level lifecycle ---
## Docker is the only host prerequisite. The peer-driver, like Core and Bridge, is built inside
## the dev container and shipped as an image; it runs in-cluster as a Kubernetes Job. No host
## Rust toolchain required.

up: cluster images
	@echo "==> testbed cluster up. next: make smoke"

cluster:
	$(BASH) $(REPO_ROOT)/testbed/kind/up.sh "$(CLUSTER_NAME)"

images:
	$(BASH) $(REPO_ROOT)/testbed/images/build-and-load.sh "$(CLUSTER_NAME)"

down:
	$(BASH) $(REPO_ROOT)/testbed/kind/down.sh "$(CLUSTER_NAME)"

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
