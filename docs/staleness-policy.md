# Stale-state verification policy (§13.4.3)

**Status:** Resolved as a **structure plus recommended defaults**. The Verifier applies a
per-use-type staleness threshold (`StalenessPolicy` in `creda-verifier`); the relying institution
keeps override authority; and the threshold *values* are bootstrap defaults to be refined per
deployment with pilot data. This closes the structural half of §13.4.3. Like the §5.3.2 match
calibration, the mechanism is real and the calibrated numbers are operational — they come from each
deployment's data, not from a universal constant.

## The problem

The Verifier runs **offline** at the point of use, against a local DAG replica that may lag the
network, and it reports how old its view is. But "how stale is too stale" is not universal:

- It is **use-dependent.** A routine read at the point of care tolerates a view that is hours
  behind — availability matters when a clinician needs data. A fresh-authorization check just before
  a **bulk export** tolerates almost none — you want to catch a just-issued revocation before data
  leaves the institution.
- It is **risk-tolerance-dependent.** Institutions differ.

A single network-wide threshold would be wrong for someone. So the network does not impose one; it
provides a per-use-type policy with recommended defaults, and the relying institution decides.

## The resolution: per-use-type policy + institutional override

`StalenessPolicy` classifies each verification request into a `UseClass` and maps it to a staleness
threshold. Classification reads signals already present on the `AuthorizationQuery`: `use_mode`,
`purpose`, and `requested_data_categories`. Staleness stays **advisory** — `verify()` reports the
classified `use_class`, the applied threshold, the view age, and a `stale` flag, and the caller
applies its own policy. The relying institution constructs the `StalenessPolicy`, so overriding any
threshold (or the sensitive-category set) is its authority, exactly as §13.4.3 requires.

## Use classes and recommended bootstrap thresholds

These are **defaults, not law.** They are deliberately conservative starting points; a deployment
overrides them.

| Use class | Recommended default | Why |
|---|---|---|
| **Pre-export** (`UseMode::ReadAndExport`) | **5 minutes** | Data is leaving the institution. You want a near-fresh view so a just-issued revocation is caught before release. In practice a stale pre-export result should prompt a fresh sync and re-check, not silently block. |
| **Sensitive read** (a 42 CFR Part 2 / behavioral-health / HIV / reproductive / genetic category) | **1 hour** | Heightened-protection data warrants tighter freshness on the governing authorization regardless of purpose. |
| **Research / AI** (`Research`, `AiTraining`, `AiInference`) | **12 hours** | The data itself tolerates staleness, but consent-revocation freshness still matters. See the tension note below. |
| **Routine read** (everything else) | **24 hours** | Point-of-care and operations reads; availability dominates, and the substantive authorization check still runs regardless. |

**Classification is most-protective-first:** export → sensitive → research/AI → routine. So an export
*of* sensitive data is classified pre-export (the tightest), not sensitive-read; a research use *on*
sensitive data is classified sensitive-read (tighter than research). The default thresholds are
monotonic in that order, so "first match" and "tightest applicable" coincide; an institution that
overrides should keep that ordering in mind.

**Research/AI tension.** Twelve hours is a middle default. An institution running high-frequency AI
*inference* against near-real-time data may want this closer to the sensitive or pre-export end (a
revoked research consent should stop new analyses promptly); a batch *training* pipeline over
historical data tolerates more. This is exactly the kind of value pilot data and institutional risk
appetite should set.

**Sensitive categories** default to a starting set of commonly-regulated labels
(`behavioral-health`, `mental-health`, `psychotherapy-notes`, `substance-use`,
`substance-use-disorder`, `part2`, `hiv`, `aids`, `reproductive-health`, `sexual-health`,
`genetic`), matched case-insensitively. Each institution maps this to its own data-category
vocabulary — that mapping is deployment configuration, not a network constant.

## What is calibrated per deployment, and how

Two things are per-deployment: the **threshold values** and the **sensitive-category vocabulary**.
Calibrating the thresholds is an operational exercise, not a code change:

1. Measure the deployment's **replication-lag distribution** (how far behind a typical replica runs)
   and its **revocation-propagation time** (how long a revocation takes to reach relying peers).
2. For each use class, set the threshold so the probability of acting on a since-revoked
   authorization within the window is acceptable **for that use** — tight where the cost of acting
   on stale state is high (export, sensitive), looser where availability dominates (routine).
3. Fold in the institution's risk tolerance; the institution retains final override authority.
4. Revisit with production data; publish updated recommended defaults as pilot evidence accumulates.

## What ships now vs. what needs data

**Ships now:** the `StalenessPolicy` structure, the `UseClass` classifier (most-protective-first
over `use_mode` / `purpose` / data categories), the recommended bootstrap thresholds, the advisory
report fields (`use_class`, `staleness_threshold_secs`), and full institutional override.

**Needs deployment data:** the calibrated threshold values and the institution's sensitive-category
mapping. Until then the recommended defaults apply — conservative, documented, and overridable.
