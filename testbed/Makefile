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

.PHONY: up down clean images smoke

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
