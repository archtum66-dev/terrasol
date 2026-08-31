# TerraSol

Cheap, verifiable proof anchoring on Solana. TerraSol checks an environmental
claim against the issuing registry, blocks double counting, and anchors the
result on-chain as a SHA-256 fingerprint. The document never leaves the
customer's machine; only the hash reaches the chain. Anyone holding the
document can verify it in a browser in seconds — and the chain refutes the
check if TerraSol's own register was tampered with.

One rule shapes the architecture: **matching does not belong on a chain,
proof does.** An off-chain Rust engine handles order flow, Solana notarises.
Anchoring one proof costs 5,000 lamports; anchoring a batch of 469,329 fills
costs the same 5,000 lamports — 0.011 lamports each. Every number in this
repository is measured, not estimated.

## Status and scope

**Nothing is deployed to mainnet and no token has been issued.** The project's
own standing rule is an external security audit before any mainnet deployment,
without exception. What exists here has been built, executed and measured on a
local validator with Metaplex cloned from mainnet.

`TRRA` is specified but not issued, and is deliberately kept out of the way of
everything else in this repository. It is a pure utility token: staking grants
access tiers and governance voting rights, with **no claim to yield, profit,
interest or dividends** and no equity. Fixed supply 100,000,000; mint and
freeze authority revoked at issuance. That wording is part of the legal design
(Swiss FINMA utility-token line), not marketing copy — keep it intact.

The commercial service that runs today is token-less: registry verification
sold per certificate, at terrasols.org.

## Architecture

The same principle Hyperliquid proved at scale, but settling on Solana
instead of running its own L1: it inherits Solana's validators, wallets and
liquidity instead of bootstrapping trust from zero.

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
