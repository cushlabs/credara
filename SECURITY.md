# Security Policy

Credara is infrastructure for handling cross-institutional patient identity and
authorization. Security issues are treated with corresponding seriousness.

## Reporting a vulnerability

**Do not open a public GitHub issue for a security vulnerability.** Public issues are
visible to everyone and can put deployments at risk before a fix is available.

Instead, report privately through one of:

- GitHub's [private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)
  for this repository (Security → Report a vulnerability), or
- email to the maintainers' private security address.

> _Maintainer note: replace this line with the project's monitored security contact
> (e.g. `security@<domain>`) before public launch._

Please include: a description of the issue, the component and spec section affected (if
known), reproduction steps, and the potential impact. If you have a suggested fix, that
is welcome but not required.

## What to expect

- **Acknowledgement** of your report within a few business days.
- An initial assessment and severity classification.
- Coordinated disclosure: we will work with you on a fix and a disclosure timeline, and
  credit you in the advisory unless you prefer to remain anonymous.

Please give us a reasonable opportunity to remediate before any public disclosure.

## Scope

In scope: the Credara Core, Export Gate, Verifier, FHIR Bridge, deployment artifacts, and
the cryptographic, networking, and authorization logic described in the
[technical specification](docs/creda-technical-spec.md) §9 (Security and Access Control).

The security model — UDAP + SPIFFE dual credentials, mandatory signature verification on
replication, authorization enforcement at the responding peer, and dual-control
(Export Gate + Verifier) — is load-bearing. Reports demonstrating a way to bypass any of
these are especially valuable.

## Out of scope

- Issues in upstream dependencies (libp2p, HAPI FHIR, RocksDB, etc.) — please report
  those to the respective projects, though we appreciate a heads-up.
- Findings that require physical access to a peer operator's infrastructure or a
  compromised operator credential (these are outside Credara's trust boundary by design;
  see the threat model in spec §9.1).

## Handling of test data

This project uses **synthetic data only**. If you encounter what appears to be real PHI,
credentials, or secrets committed to the repository, treat it as a security incident and
report it privately through the channels above.
