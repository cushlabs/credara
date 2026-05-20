# creda-export-gate (M6)

Source-side half of dual-control authorization enforcement.

**Governing spec sections:** §4.5 (Dual-Control Enforcement), §10.2 (Export Gate).

Will contain: validation of a Portable Authorization Artifact before data egress; emission of an
ExportReceipt (chain of custody); the egress-hook integration points.

**Assemble:** reuse `creda-graph`'s authorization evaluation — do NOT reimplement it.
**Write:** the egress-hook integration surface. `TODO(open-question-13.4)` Export Gate integration patterns.

Not yet registered as a Cargo workspace member; added in M6.
