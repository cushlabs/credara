# Design Note — DHT Query-Privacy (closure plan for spec §13.3)

**Status:** design proposal, not implemented. Security-relevant; the cryptographic options below
(OPRF, PIR/PSI) **require review by a cryptographer before shipping** — do not hand-roll. Expands
spec **§9.5 (Future Privacy Enhancements)** and proposes a concrete closure path for the §13.3
open question. See also `docs/STATUS.md` (security flags) and `docs/PILOT.md` (the pilot "DHT-off"
decision).

## The problem (and why lookups leak at all)

Credara uses a Kademlia DHT for **discovery**: a `token → [peer IDs that hold this subgraph]` record
(§6.1.6; token = salted hash of demographics, §9.2). No single peer holds the full index — each
record lives on the peers whose node-IDs are XOR-closest to the token. So a lookup is **iterative
and multi-hop**: the querier asks progressively-closer peers "who do you know nearer to key T?"
until it reaches the responsible nodes. **Every peer on that path learns `(querier, key T)`.** Two
leak surfaces:

1. **Query privacy** — path/terminal nodes learn *who is asking about which key*. Tokens are
   deterministic within a salt epoch, so a curious peer can (a) correlate "A keeps asking about T"
   over time, and (b) if it knows a patient's demographics, **precompute T offline and confirm**
   "is anyone looking this person up?"
2. **Value privacy** — the record itself reveals *which institutions hold that patient* (the care
   graph), to anyone who stores or fetches it.

**Already mitigating:** cleartext PHI never traverses; tokens are salted and rotate annually
(§9.2.2); and — critically — this is **not an open DHT**: admission control bounds Sybil (you can't
cheaply position thousands of nodes across the keyspace; each node needs an admitted institutional
identity). The residual adversary is a **malicious or curious admitted institution**.

## Why we can't just delete the DHT

The DHT is load-bearing for **first-encounter cross-institution linking**: a newly-admitted
institution registers a patient and must discover whether anyone else already holds that person,
to create a `Link`. The alternative — subscribe to bucketed gossip and match locally — only covers
the general-matching case if you subscribe to *every* bucket (≈ full replication), which defeats
the point. So: keep the DHT, make its lookups private.

## Cost model — bucket-coarsened lookups

"Coarsen the key": look up the **bucket** (1 of `B`=1024) instead of the exact patient token, then
filter locally — so the DHT sees only bucket-level interest (anonymity set ≈ bucket occupancy).
Cost splits in two:

- **The lookup itself: unchanged.** Still one iterative Kademlia lookup, ~`log(N)` hops for `N`
  peers. Coarsening changes the *key*, not the hop count.
- **Downstream transfer: scales with bucket occupancy.** To match locally without re-leaking the
  exact token you pull the bucket's contents:

  ```
  transfer per link-discovery ≈ (P / B) × events_per_patient × holders_of_bucket
  ```

  where `P` = total patients, `B` = bucket count. That `P/B` factor **is also the anonymity set**
  `k`.

Worked numbers:
- **Early testing** (P≈10²–10³, B=1024): occupancy `P/B` ≈ 0.1–1 → a coarsened lookup returns ~the
  same as exact. **Cost ≈ exact; coarsening is nearly free — but buys ~no privacy** (k≈1).
- **Production** (P=1M, B=1024, ~10 events/patient): ~10k events pulled per discovery vs ~10 exact
  → **~1000× transfer inflation**, for a ~1000-patient anonymity set.

**Key insight:** bucket-coarsening's cost and its protection are the *same number* (`P/B`); `B` just
slides you along that curve. It's cheap-but-useless early, strong-but-expensive later. For the
*linking* use case (which wants cheap targeted pulls), coarsening is therefore the wrong primary
lever.

## Mitigation menu (by what each protects, cheap → exotic)

| Mechanism | Protects | Cost | Notes |
|---|---|---|---|
| **Coarsen key (bucket lookup)** | which-patient (k-anonymity) | transfer ∝ `P/B` | free at small scale; `B` is the dial; use opportunistically for sensitive lookups |
| **OPRF-blinded keys** | precompute + cross-epoch correlation | per-lookup crypto op | key = OPRF(token) under a network key; node can't reverse to demographics or precompute. **Keeps cheap exact-token pulls** — best ROI for linking |
| **Relay / onion via admitted peers** | who is asking (origin) | +1–2 hops latency | terminal node sees a relay, not the origin |
| **PIR / PSI** | full query obliviousness | high (crypto + non-collusion) | spec §9.5 endgame; PIR = fetch a record without the holder learning which; PSI = learn only shared patients |
| **Cover traffic / decoys** | obscure real interest | bandwidth | partial; raises adversary cost |
| **Audit + rate-limit lookups** | accountability (detect/deter) | low | leverages admission + immutable DAG (cf. §8.2.10.3); defense-in-depth, not a fix |

## Recommended roadmap

1. **Pilot (now):** synthetic/closed network → the leak is harmless. Either run the DHT as-is or
   leave discovery on bucket-gossip; **measure** actual lookup fan-out and bucket occupancy at test
   scale. Those numbers drive the `B` and OPRF decisions. (Recorded as the pilot "DHT-off / measure"
   decision in PILOT.md.)
2. **Near-term (real data):** **OPRF-blinded exact-token lookups + relay.** Preserves cheap,
   targeted linking while removing the precompute/correlation attacks (query content) and hiding
   the querier (origin). This is the practical fix for the linking use case.
3. **Opportunistic:** bucket-coarsening for explicitly-sensitive lookups, tuned via `B`.
4. **Endgame:** PIR/PSI per §9.5 for full obliviousness, once warranted by scale/threat.

## Open crypto questions (for the reviewer)

- **OPRF key custody:** who holds the OPRF key? A single holder is a chokepoint/trust anchor and
  can itself observe queries; threshold/distributed OPRF avoids that but adds protocol. Rotation and
  its interaction with the demographic-salt epoch (§9.2.2) need design.
- **PIR assumptions:** single-server PIR is expensive; multi-server needs non-colluding servers —
  does the admitted-institution trust model supply that?
- **Value-privacy:** the menu mostly addresses *query* privacy; the record still maps token→holders.
  Encrypting records to authorized requesters (or storing only blinded holder hints) is a separate
  sub-problem.

## Closure condition (proposed for §13.3)

Measure lookup/occupancy in the pilot; design + cryptographer-review the OPRF+relay scheme; pin
`B`; document the residual (curious-admitted-institution) threat and the value-privacy gap. Promote
PIR/PSI to a tracked §9.5 work item. Re-evaluate before any real-PHI deployment — this is a **hard
gate** for PHI (see PILOT.md).
