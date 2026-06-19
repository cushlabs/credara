# Match & confidence calibration (§5.3.2) — resolution

This closes the §5.3.2 open question. It does **not** ship a set of magic numbers — it commits to a
calibration *methodology*, because the weights are properties of a population and its source systems,
not universal constants. Hand-set weights are a heuristic; a hospital review asks "how were these
derived and validated," and in matching a mis-weight is wrong-patient linkage (a safety event). What
follows is the defensible, auditable process, plus the bootstrap defaults the code ships with so that
day one is not cold.

Two distinct models share this open question:

- **Match scoring** (record linkage — "are these two records the same person?"): `PatientMatcher`,
  behind `Patient/$match`. Fellegi–Sunter.
- **Confidence scoring** (field reliability — "how much do I trust this asserted value?"):
  `creda-graph::confidence`, behind the effective-identity projection. Additive evidence + decay.

Both are calibrated per deployment; neither is calibrated by a constant in the source tree.

## 1. Match scoring — Fellegi–Sunter

For each comparison field there are two probabilities:

- **m** = P(field agrees | the two records are the *same* person) — the source systems' error/change
  model (typos, nicknames, capture quality, missingness, staleness).
- **u** = P(field agrees | the two records are *different* people) — essentially the value's frequency
  in the population (agreeing on "Smith" is weak, "Zbigniew" strong).

Agreement weight = `log2(m / u)`, disagreement weight = `log2((1 − m) / (1 − u))`. A candidate's score
is the sum of its field weights — a base-2 log-likelihood ratio (LLR). Decision thresholds on the LLR
sort candidates into grades (`certain` / `probable` / `possible` / `certainly-not`); the FHIR
`search.score` is a monotone 0–1 transform of the LLR for display.

**Frequency-based u is the single biggest accuracy lever** and is required, not optional. u for a
*value* approximates its relative frequency in that field's population, so a match on a rare value
weighs far more than a match on a common one. The scorer exposes a `UProbability` hook for a per-value
frequency table; absent one it falls back to the field-level u (functional, but under-weights rare
agreements — bootstrap only).

### Calibrating m, u, and the thresholds (per deployment)

1. **u** — estimate from the deployment's own value-frequency distributions (a frequency table per
   field). These differ by population (region, demographics, language) and must be local.
2. **m** — estimate from labeled match/non-match pairs if available (supervised); otherwise via the
   **EM algorithm** (unsupervised) over the observed agreement-pattern vectors. Bootstrap the priors
   from the published HIE/MPI literature (e.g. Regenstrief/Indiana HIE matching studies; ONC / Sequoia
   Project patient-matching work) so the first EM iteration starts from sane values.
3. **Thresholds** — set the upper/lower LLR cut-points to meet *target error rates* (false-match ≤ μ,
   false-non-match ≤ λ) measured against a **gold-standard labeled sample**; the band between is the
   `possible` / clerical-review zone. So `0.80/0.50`-style numbers become *derived* from a stated
   target like "false-match ≤ 0.1%."

### Why this is per-deployment and needs real data

m, u, and the thresholds are all functions of the local population and source systems, so a calibration
from Hospital A mis-weights at Hospital B (false matches on agreements common in B; missed matches on
rare-but-discriminating ones). You cannot *assert* an error rate for B without measuring it on B's data.
Real data is required specifically because: u needs actual value frequencies, m / EM need realistic
error patterns, and the threshold validation that certifies an error rate needs records with known truth.
Synthetic data validates the machinery and the math — it cannot certify a production error rate, because
its frequencies and error patterns are not the real population's.

## 2. Confidence scoring — same discipline, different model

`creda-graph::confidence` is not a record-linkage problem; it scores how reliable a single asserted
field *value* is, from verification-method weight, institutional credibility, independent-agreement and
independent-attestation amplification, and temporal decay, squashed by a saturating function
`conf = T/(T+K)` for diminishing returns. Its weights, the saturation constant `K`, and the decay curves
are already network configuration (`MethodWeights`, `Default`) — they are calibrated by the same
per-deployment discipline: bootstrap defaults from the literature, tuned against the deployment's data
and a validation set, versioned and auditable. It is **not** restructured into the m/u form — the models
are distinct; only the calibration *process* is shared.

## 3. The operational close

Calibration is a versioned, auditable artifact, not a code constant:

- Weights / thresholds (match) and method-weights / K / decay (confidence) load at runtime from a
  per-deployment calibration artifact; the source-tree values are clearly-labeled **bootstrap priors**.
- Each deployment runs the calibration (frequency tables + EM / labeled m + threshold-to-target-error)
  and validates against a gold-standard sample **before go-live**; the achieved false-match /
  false-non-match rates are recorded.
- The artifact is re-estimated on a schedule and the score distribution monitored for drift; a material
  shift triggers re-calibration.
- The whole artifact (inputs, method, validation results, version) is retained for audit — a reviewer
  can see exactly how a given match decision was weighted.

## 4. What ships now vs. needs the deployment's data

| Piece | State |
|---|---|
| FS match model (log-weights, frequency-`u` hook, LLR → grade) | ✅ in `PatientMatcher` |
| Confidence evidence model (method/agreement/attestation/decay) | ✅ in `creda-graph::confidence` |
| Loadable calibration artifact + bootstrap-prior defaults | ✅ (defaults shipped; load path is the integration seam) |
| Per-value frequency tables; EM / labeled m estimation; threshold-to-error-rate validation | ⏳ per-deployment, needs real (or realistically-distributed) data |

So §5.3.2 is **resolved as a methodology**: the models and the loadable structure are production-ready;
the calibrated numbers and their validated error rates are a defined, auditable per-deployment step,
which is the correct and only honest way to set them.
