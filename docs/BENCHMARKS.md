# Benchmarks — measured, never estimated

Hardware: 2-core Intel Xeon 2.80 GHz container. Modern server hardware with
AVX2 runs the signature paths ~2–4× faster. Reproduce with the commands shown;
raw outputs live in the project archive (`_projekt/messung*.txt`).

## Matching engine (in-memory, single core)

`cargo run --release --bin messung -- bench` — 3M-operation stream,
55% place / 35% cancel / 10% take (the mix real venues see).

| Metric | Value |
|---|---|
| Throughput | **11,827,313 ops/s** (best), 11.3M mean |
| Per operation | **85 ns** |
| Book state after 3M ops | 533,966 resting orders, never crossed |

## Full pipeline over real TCP (2 cores)

`cargo run --release --bin netz` — framing, ed25519 verification, ordering,
matching, group-commit log.

| Configuration | Orders/s |
|---|---:|
| signature per order, single verify | 51,921 |
| signature per order, 2 verifiers | 99,851 |
| **bundle of 16 orders, one signature** | **575,390** |
| bundle of 64 | 2,052,262 |
| **bundle of 256** | **5,055,457** |

Where the time goes (per order, batched verify): signature **97.2%**,
framing 0.1%, matching 0.2%, group-commit log 2.6%. The order book is never
the bottleneck; per-order signatures are. Bundling — one signature over n
orders, exactly what Hyperliquid's own `bulk_orders` does — is the lever:
**134×** measured end to end.

## Settlement on Solana

`python3 program/settlement_demo.py` — real engine run, state hashed,
anchored on a local validator, read back and verified.

| Metric | Value |
|---|---|
| Anchor cost per batch | 5,000 lamports (one flat fee) |
| At 469,329 fills per batch | **0.0107 lamports ≈ 0.000001 USD per fill** |
| Proof-of-impact anchor (oracle path) | 5,000 lamports ≈ **0.000475 USD** per proof |

## Smart contract (local validator, Agave 4.2.1)

`bash program/alles-testen.sh` — fresh chain → deploy → 15-check e2e:
initialize/config readback, stake→tier 2, lock enforcement (6003), oracle
authorisation (6009), impact record readback, marketplace list/buy with real
TRRA transfer, governance gate (6010), pause semantics (6002). **15/15.**

Program size 320,280 bytes → mainnet rent ≈ 2.23 SOL (≈ 212 USD at SOL 95).

## Why settle on Solana instead of running an own L1

Measured here: a chainless engine does 11.8M ops/s; adding per-order
signatures costs 97% of everything; consensus is not in these numbers at all.
Hyperliquid pays that consensus price with its own validator set (~200k
orders/s). For a verification marketplace the chain only needs to *prove*,
not *match* — so Solana as notary keeps the throughput of the engine and the
security of an established validator set, for lamports.
