package health.creda.bridge.providers

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/**
 * `$match` is real Fellegi–Sunter scoring, not a fabricated score: full agreement is certain, a
 * disagreement on a heavy field (DOB) drives the score negative, a heavier field outweighs a lighter
 * one, missing candidate fields are neutral, and an empty query carries zero evidence. (Assertions are
 * on qualitative properties, so they hold as the bootstrap weights are recalibrated.)
 */
class PatientMatcherTest {

    private fun llr(query: Map<String, String>, candidate: Map<String, String>) =
        PatientMatcher.logLikelihoodRatio(query, candidate)

    @Test
    fun `full agreement is certain`() {
        val q = mapOf(
            "name-family" to "tok:demo:whitfield",
            "date-of-birth" to "tok:demo:1971-08-04",
            "sex" to "tok:demo:female",
        )
        assertEquals("certain", PatientMatcher.grade(llr(q, q)))
        assertTrue(PatientMatcher.score01(llr(q, q)) > 0.99)
    }

    @Test
    fun `a DOB disagreement outweighs a sex agreement`() {
        val q = mapOf("date-of-birth" to "tok:demo:1971-08-04", "sex" to "tok:demo:female")
        val c = mapOf("date-of-birth" to "tok:demo:1990-01-01", "sex" to "tok:demo:female")
        assertTrue(llr(q, c) < 0.0, "a DOB mismatch must drive the LLR negative")
        assertEquals("certainly-not", PatientMatcher.grade(llr(q, c)))
    }

    @Test
    fun `a heavy field outweighs a light one`() {
        val q = mapOf("date-of-birth" to "tok:demo:1971-08-04", "sex" to "tok:demo:female")
        val dobOnly = llr(q.filterKeys { it == "date-of-birth" }, q)
        val sexOnly = llr(q.filterKeys { it == "sex" }, q)
        assertTrue(dobOnly > sexOnly, "DOB agreement ($dobOnly) must outweigh sex agreement ($sexOnly)")
    }

    @Test
    fun `missing candidate fields are neutral`() {
        val q = mapOf("name-family" to "tok:demo:smith", "sex" to "tok:demo:male")
        val c = mapOf("name-family" to "tok:demo:smith") // agrees on the heavy field, lacks the light one
        assertTrue(llr(q, c) >= 0.0)
        assertEquals("probable", PatientMatcher.grade(llr(q, c)))
    }

    @Test
    fun `empty query has zero evidence`() {
        assertEquals(0.0, PatientMatcher.logLikelihoodRatio(emptyMap(), mapOf("sex" to "tok:demo:female")))
    }
}
