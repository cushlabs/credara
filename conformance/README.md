# conformance — Conformance Suite + Synthetic Data (M9)

**Governing spec sections:** §11.4 (Integration Testing in Production), §11 (Operations).

Will contain: automated validation across deployment conformance, FHIR behavior, authorization
flows, provenance preservation, revocation enforcement (incl. the Bound-1 latency check from
§4.7), and data-category handling (clinical payloads never enter the trust graph; authorization
artifacts minimized/scoped; identity assertions tokenized). Plus the synthetic data generator
(public-domain demographic corpora, realistic event chains, configurable scale, deterministic
seed) with `test-data` extension tagging so synthetic events propagate but are filtered from
clinical responses.

**Assemble:** standard test frameworks; public-domain name/address corpora.
**Write:** the conformance harnesses, the synthetic generator, the test-data tagging/filtering.
