# TerraSol

Independent CO₂ verification on Solana: oracle-signed proof-of-impact records,
tiered access via staking, and a marketplace for verified impact credits —
with an off-chain matching engine and on-chain settlement, so throughput is
never limited by the chain.

**TRRA** is a pure utility token: staking grants access tiers and governance
voting rights. It carries **no claim to yield, profit, interest or dividends**
and represents no equity. Fixed supply 100,000,000; mint and freeze authority
revoked at issuance. That wording is part of the legal design (Swiss FINMA
utility-token line), not marketing copy — keep it intact.

## Architecture

The design follows the principle proven by Hyperliquid — matching does not
belong on a chain, proofs do — but settles on Solana instead of running its
own L1, so it inherits Solana's validators, wallets and liquidity instead of
bootstrapping trust from zero.

```
verify (oracle)          match (engine)             settle & prove (Solana)
────────────────         ──────────────             ───────────────────────
Gold Standard /          in-memory CLOB             terrasol program:
Verra registry lookup    11.8M ops/s single core    stake / tiers / impact
retired-only, no         5.06M orders/s over TCP    records / marketplace
double counting          (batched ed25519)          + SHA-256 batch anchors
```

Every number above is measured, not estimated — see `docs/BENCHMARKS.md`.

## Repository layout

| Path | What | Status |
|---|---|---|
| `program/` | Anchor smart contract: staking tiers, oracle-signed impact records, credit marketplace, governance | compiled, deployed and **15/15 e2e-tested** on a local validator |
| `engine/` | Rust limit order book (price-time priority), HIP-1/HIP-2 reimplementation, throughput benchmarks | 15/15 unit tests, benchmarked |
| `oracle/` | Verification pipeline (registry lookup, anti-double-counting ledger), on-chain anchoring, public check page | 29/29 tests incl. real-browser run |
| `token/` | TRRA mint tooling (SPL + Metaplex metadata, hand-built, no IDL dependency) | verified against mainnet programs |
| `docs/` | Roadmap, benchmarks | – |

Program ID (local/devnet): `3GGT5oAJXjpvFnofn3W25jTBhKRp4TEmKSSyzm7J7E9z`
The mainnet ID will differ and will be pinned here after the audit.

## Quick start

Requires Rust, Node 22+, Python 3.11+ and the Agave toolchain
(`solana-test-validator`, `cargo build-sbf`).

```bash
# smart contract: fresh chain -> deploy -> full end-to-end test (15 checks)
cd program && bash alles-testen.sh

# engine: unit tests + throughput measurement
cd engine && cargo test --release && cargo run --release --bin messung

# oracle: verification pipeline + on-chain anchor + check page (29 checks)
cd oracle && npm install && npm test     # needs a running local validator

# the whole concept in one run: engine matches off-chain,
# the chain notarises the result for one flat fee
cd program && python3 settlement_demo.py
```

## Security

- Checked arithmetic everywhere; overflow aborts, never wraps.
- Oracle is a single signer for the pilot, rotatable via governance
  (`set_oracle`); production key belongs in a KMS/HSM.
- Vault is a PDA; unstake enforces a 7-day lock.
- Only SHA-256 hashes ever reach the chain — no serials, no customer data
  (GDPR/revDSG by construction).
- `docs/THREAT-MODEL.md`, `SECURITY.md` and `docs/AUDIT-READINESS.md` in
  `program/` — **an external audit and bug bounty gate any mainnet deploy.**

## Licence

Apache-2.0 (proposed — required for the Solana Foundation public-good grant
track; final call rests with the project owner).
