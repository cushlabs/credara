package health.creda.bridge.providers

import kotlin.math.log2
import kotlin.math.pow

/**
 * Patient match scoring (§5.3) behind `Patient/$match`, as Fellegi–Sunter record linkage. Each
 * comparison field carries two probabilities: `m` = P(field agrees | same person) and `u` =
 * P(field agrees | different people). Agreement contributes `log2(m/u)`, disagreement
 * `log2((1-m)/(1-u))`; a field the candidate does not surface is neutral. The per-candidate sum is a
 * base-2 log-likelihood ratio (LLR); thresholds on the LLR yield FHIR match grades, and [score01] is
 * a monotone 0–1 transform of the LLR for `search.score`.
 *
 * The weights and thresholds are a **per-deployment calibration**, not universal constants — `u` is a
 * population's value frequencies, `m` is the source systems' error model, and the thresholds are set
 * to a target error rate against a gold-standard sample. See `docs/matching-calibration.md` (§5.3.2).
 * [Calibration.BOOTSTRAP] below is literature-style priors so day one is not cold; production loads a
 * calibrated artifact and a per-value frequency table via [UProbability]. The *mechanism* is real —
 * scoring is over actual token agreement, never fabricated — and only sharpens once calibrated.
 */
object PatientMatcher {
    /** Per-field agreement probabilities. `m` = P(agree | match); `u` = P(agree | non-match). */
    data class FieldWeights(val m: Double, val u: Double)

    /**
     * A calibration: per-field [FieldWeights] (+ a fallback) and the LLR cut-points for each grade.
     * Loaded per deployment; [BOOTSTRAP] is bootstrap priors, not a calibrated set.
     */
    data class Calibration(
        val fields: Map<String, FieldWeights>,
        val fallback: FieldWeights,
        val certainLlr: Double,
        val probableLlr: Double,
        val possibleLlr: Double,
    ) {
        fun weights(field: String): FieldWeights = fields[field] ?: fallback

        companion object {
            /** Literature-style bootstrap priors (NOT calibrated). See docs/matching-calibration.md. */
            val BOOTSTRAP = Calibration(
                fields = mapOf(
                    "mrn" to FieldWeights(m = 0.99, u = 0.0001), // issuing-institution MRN, near-unique
                    "date-of-birth" to FieldWeights(m = 0.95, u = 0.003),
                    "name-family" to FieldWeights(m = 0.90, u = 0.01),
                    "name-given" to FieldWeights(m = 0.90, u = 0.02),
                    "address" to FieldWeights(m = 0.70, u = 0.01),
                    "sex" to FieldWeights(m = 0.98, u = 0.50), // ~half agree by chance: weak
                ),
                fallback = FieldWeights(m = 0.80, u = 0.10),
                certainLlr = 8.0,
                probableLlr = 4.0,
                possibleLlr = 0.5,
            )
        }
    }

    /**
     * Frequency-adjusted `u` for a field/value (a rare value lowers `u`, so a match on it weighs
     * more). Defaults to the field-level `u`; production overrides with a per-value frequency table.
     */
    fun interface UProbability {
        fun of(field: String, value: String, base: Double): Double
    }

    private val FIELD_LEVEL_U = UProbability { _, _, base -> base }

    /** Fellegi–Sunter total weight (base-2 LLR) of a [candidate]'s field tokens against the [query]'s. */
    fun logLikelihoodRatio(
        query: Map<String, String>,
        candidate: Map<String, String>,
        cal: Calibration = Calibration.BOOTSTRAP,
        uFor: UProbability = FIELD_LEVEL_U,
    ): Double {
        var llr = 0.0
        for ((field, queryToken) in query) {
            val w = cal.weights(field)
            val candidateToken = candidate[field] ?: continue // candidate lacks the field -> neutral
            llr += if (candidateToken == queryToken) {
                log2(w.m / uFor.of(field, queryToken, w.u)) // agreement
            } else {
                log2((1.0 - w.m) / (1.0 - w.u)) // disagreement
            }
        }
        return llr
    }

    /** FHIR match grade for an LLR (CodeSystem `http://terminology.hl7.org/CodeSystem/match-grade`). */
    fun grade(llr: Double, cal: Calibration = Calibration.BOOTSTRAP): String = when {
        llr >= cal.certainLlr -> "certain"
        llr >= cal.probableLlr -> "probable"
        llr >= cal.possibleLlr -> "possible"
        else -> "certainly-not"
    }

    /** Monotone 0–1 transform of the LLR for FHIR `search.score` (a display confidence, not the decision). */
    fun score01(llr: Double): Double = 1.0 / (1.0 + 2.0.pow(-llr))
}
