# Scenario: rogue-link

Exercises the **cross-institutional link-chain defense** (§4.6 step 5.5, method ceilings §5.3.5)
over the real libp2p mesh: a rogue institution gossips a hostile identity fragment and a
self-issued Grant onto a patient it does not legitimately know, and the responding peer refuses to
honor it — while an otherwise-identical fragment carried by a *trusted* Link is honored.

`make rogue-link`

## The attack it models

A Grant only means something relative to a patient the responder actually holds. So a rogue
institution cannot simply issue a Grant naming itself — it has to connect that Grant to the
responder's patient. The only join primitive is a `Link` (§5.1.1). The attack is therefore: author
a parallel Assert, self-issue a Grant on it, then fuse that fragment onto the real patient with a
Link you control, claiming maximum confidence.

The defense (§4.6 step 5.5): when the responder evaluates authorization, a Grant that reaches its
patient *only* by crossing a Link is trusted no more than that Link's **method** allows. Each method
has a confidence ceiling (§5.3.5); a `manual` link is capped well below the trust floor, so however
high its stated confidence, it cannot carry a Grant across the institutional boundary. The
responder's own Asserts/Attests are the anchors that need no Link.

## What it does

Two peers, meshed (peer-b bootstraps to peer-a). **peer-b is the responder and runs
`deny-by-default`** — under `treatment-presumed` a Treatment request is authorized regardless of
Grants, which would make the Grant (and the whole defense) moot; deny-by-default makes the Grant the
sole path to a yes, so the link-chain verdict is decisive.

1. **Real patient** — peer-b authors the patient's Assert. Because peer-b signed it, it is a
   responder anchor.
2. **Rogue fragments** — peer-a injects two parallel Asserts it controls and fuses each onto the
   real patient:
   - `rogue1` via a **manual** Link at confidence 10000 (capped below the floor);
   - `rogue2` via an **insurance-crosswalk** Link at 9500 (clears the floor).
3. **Self-issued Grants** — peer-a issues a Grant on each rogue fragment, with **distinct audience
   classes** (`rogue-tpo`, `crosswalk-tpo`) so each `check-authz` isolates exactly one Grant.
4. **Replicate** — wait until both Grants have gossiped peer-a → peer-b.
5. **Verdict at peer-b** — `EvaluateAuthorization` twice:
   - requester in `rogue-tpo` → **DENIED** (the manual-Link-reached Grant has no standing);
   - requester in `crosswalk-tpo` → **AUTHORIZED** (the crosswalk-Link-reached Grant is admitted).

Same patient, same responder, same self-issued shape — only the Link method differs. That is what
makes it a controlled test: the defense rejects the rogue path specifically, not cross-institutional
links in general.

## What success looks like

```
==> [peer-b] injecting the real patient Assert (the responder's trusted anchor)
    real patient = 0190...
==> [peer-a] fusing rogue1 → real with a MANUAL Link @10000 (capped below the floor)
==> [peer-a] fusing rogue2 → real with an INSURANCE-CROSSWALK Link @9500 (clears the floor)
==> [peer-a] self-issuing a Grant on each rogue fragment (distinct audience classes)
==> confirming both Grants have replicated peer-a → peer-b
    both grants present at peer-b
==> [peer-b] EvaluateAuthorization — rogue class must be DENIED (§4.6 step 5.5)
    rogue request → denied (reason: ...)
==> [peer-b] EvaluateAuthorization — control class must be AUTHORIZED
    control request → authorized (reason: ...)
PASS: rogue-link (manual-Link-reached Grant DENIED; crosswalk-Link-reached Grant AUTHORIZED; §4.6 step 5.5)
```

## Prerequisite

Uses the `inject-link` and `check-authz` peer-driver subcommands added alongside this scenario, and
the link-chain check wired into the Core's `EvaluateAuthorization`. Rebuild the image:

```
make up          # or: make images
make rogue-link
```

## Reading a failure — which layer it points at

- **Rogue request is AUTHORIZED.** The defense did not deny the manual-Link-reached Grant. Either
  the link-chain check is not wired into `evaluate_authorization` (Core regression — cross-check the
  `link_chain_denies_grant_reached_only_through_rogue_link` unit test in `crates/creda-core`), or the
  method ceiling / trust floor was changed so a manual link now clears it (§5.3.5).
- **Control request is DENIED.** The defense is too aggressive — it is rejecting a trusted
  cross-institutional link, not just the rogue one. Suspect the insurance-crosswalk ceiling or the
  confidence floor. The single-process link-chain unit tests isolate this.
- **A Grant never replicates to peer-b.** Not defense-specific — the mesh or ingest is the fault.
  Cross-check `make smoke`.
- **Driver Job errors with an unrecognized subcommand.** Stale `peer-driver:testbed` image —
  rebuild (`make up`).

## Relationship to the in-process suite

The single-process conformance suite tests the link-chain *logic* against `MockTransport`, and
`crates/creda-core` has deterministic unit tests for the *wiring* (a foreign-signed rogue fragment
ingested into one Core, denied on evaluation). This scenario adds what a single process cannot: the
rogue fragment arriving at the responder over **real gossip** from a separate signed peer, and the
responder's real gRPC `EvaluateAuthorization` returning the verdict.
