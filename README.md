# TerraSol

Cheap, verifiable proof anchoring on Solana. TerraSol checks an environmental
claim against the issuing registry, blocks double counting, and anchors the
result on-chain as a SHA-256 fingerprint. The document never leaves the
customer's machine; only the hash reaches the chain. Anyone holding the
document can verify it in a browser in seconds — and the chain refutes the
check if TerraSol's own register was tampered with (the check page only
accepts successful anchor transactions paid for by TerraSol's own keys,
configured via `ANCHOR_SIGNERS`).

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
| `program/` | Anchor smart contract: staking tiers, oracle-signed impact records, credit marketplace, two-step governance | native test suite over every instruction (incl. unstake after the 7-day lock, controlled clock) + end-to-end run on a local validator |
| `engine/` | Rust limit order book (price-time priority), HIP-1/HIP-2 reimplementation, throughput benchmarks | unit tests incl. overflow and memory bounds, benchmarked |
| `oracle/` | Verification pipeline (registry lookup, race-free anti-double-counting ledger), on-chain anchoring, public check page | offline + hardening tests on every push; chain and check-page tests on a local validator |
| `token/` | TRRA mint tooling (SPL + Metaplex metadata, hand-built, no IDL dependency) | verified against mainnet programs |
| `docs/` | Roadmap, benchmarks | – |

Program ID (local/devnet): `3GGT5oAJXjpvFnofn3W25jTBhKRp4TEmKSSyzm7J7E9z`
The mainnet ID will differ and will be pinned here after the audit.

## Quick start

Requires Rust, Node 22+, Python 3.11+ and the Agave toolchain
(`solana-test-validator`, `cargo build-sbf`). `bash scripts/setup-toolchain.sh`
installs all of it (pinned to the version CI uses). Everything below runs on a
local chain with play money — no SOL, no costs. All commands from the repo root:

```bash
# smart contract: native tests of every instruction (no validator needed)
cargo test --manifest-path program/Cargo.toml

# smart contract: build -> fresh local chain -> end-to-end test
bash program/alles-testen.sh

# engine: unit tests + throughput measurement
cargo test --release --manifest-path engine/Cargo.toml
cargo run --release --manifest-path engine/Cargo.toml --bin messung

# oracle: offline + hardening tests, then chain + check page
(cd oracle && npm install && npm run test:offline)
(cd oracle && npm test)                  # needs the local chain from above

# the whole concept in one run: engine matches off-chain,
# the chain notarises the result for one flat fee
python3 program/settlement_demo.py
```

CI runs all of the above on every push, including the chain suites.

## Security

- Checked arithmetic in program and engine, plus `overflow-checks = true`
  in both release profiles: overflow is rejected or aborts, never wraps.
- Only the program's upgrade authority can `initialize` — no front-running
  of the singleton config after deploy.
- Governance moves in two steps (propose, then accept by the new key).
- Oracle is a single signer for the pilot, rotatable via governance
  (`set_oracle`); production key belongs in a KMS/HSM.
- Vault is a PDA; unstake enforces a 7-day lock and is never blocked by
  `paused` — principal in == principal out.
- Marketplace purchases carry a `max_price`: a re-listing at a higher price
  between signing and execution makes the purchase fail.
- The public check page runs under a strict Content-Security-Policy, never
  renders untrusted input as HTML, and cannot be crashed by a request.
- Only SHA-256 hashes ever reach the chain — no serials, no customer data
  (GDPR/revDSG by construction).
- `SECURITY.md`, plus `program/docs/THREAT-MODEL.md` and
  `program/docs/AUDIT-READINESS.md` — **an external audit and bug bounty gate
  any mainnet deploy.**

## Licence

Apache-2.0 (proposed — required for the Solana Foundation public-good grant
track; final call rests with the project owner).
