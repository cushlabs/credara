# Creda testbed — top-level targets.
#
# Bring up a kind cluster, build & load the Core/Bridge images, run scenarios, tear down.

SHELL := /usr/bin/env bash
.SHELLFLAGS := -eu -o pipefail -c

CLUSTER_NAME ?= creda-testbed
KIND         ?= kind
KUBECTL      ?= kubectl
HELM         ?= helm

REPO_ROOT := $(realpath $(dir $(lastword $(MAKEFILE_LIST)))/..)

.PHONY: up down clean images smoke ae-repair

## --- top-level lifecycle ---
## Docker is the only host prerequisite. The peer-driver, like Core and Bridge, is built inside
## the dev container and shipped as an image; it runs in-cluster as a Kubernetes Job. No host
## Rust toolchain required.

up: cluster images
	@echo "==> testbed cluster up. next: make smoke"

cluster:
	$(REPO_ROOT)/testbed/kind/up.sh "$(CLUSTER_NAME)"

images:
	$(REPO_ROOT)/testbed/images/build-and-load.sh "$(CLUSTER_NAME)"

down:
	$(REPO_ROOT)/testbed/kind/down.sh "$(CLUSTER_NAME)"

clean: down
	@rm -rf $(REPO_ROOT)/testbed/.run
	@echo "==> testbed cleaned"

## --- scenarios ---

smoke:
	$(REPO_ROOT)/testbed/scenarios/gossip-convergence/run.sh "$(CLUSTER_NAME)"

# Anti-entropy repair: verifies §6.1.8 (AE backstop for gossip). Late-joining peer catches up
# via periodic manifest exchange + delta fetch. Slower than smoke (75s budget) — AE_INTERVAL_SECS
# is 30s today and the first useful round is at ~t=30s after the joining peer's daemon starts.
ae-repair:
	$(REPO_ROOT)/testbed/scenarios/anti-entropy-repair/run.sh "$(CLUSTER_NAME)"
