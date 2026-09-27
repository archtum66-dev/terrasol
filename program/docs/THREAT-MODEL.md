# Threat Model

## Assets
- Staked TRRA held in the program vault.
- Integrity of impact attestations (the product's core value).
- Governance control (config, oracle key, pause).

## Actors & trust
- **User** — untrusted. Can call stake/unstake for their own position only.
- **Oracle/Verifier** — trusted-but-rotatable. Signs impact records off-chain
  proofs. Compromise = false impact data, but cannot drain the vault.
- **Governance (Realms DAO)** — most privileged. Can pause, rotate oracle,
  change thresholds, transfer governance. Should be a multisig/DAO, never one EOA.

## Key risks & mitigations
| Risk | Vector | Mitigation |
|------|--------|-----------|
| Vault drain | Forged unstake / bad CPI signer | Transfers signed by the config PDA (the vault's token authority), owner+mint checks, checked math; tested incl. unstake after the lock |
| Config capture at deploy | First caller of `initialize` picks governance/oracle/mint | Only the program's upgrade authority may initialize (ProgramData check) |
| Governance lost to a typo | One-step transfer to a wrong key | Two steps: `set_governance` proposes, the new key must `accept_governance` |
| Marketplace price switch | Seller cancels and re-lists higher before the buy executes | `buy_credit(max_price)`: purchase fails above the signed limit |
| Funds frozen by governance | `paused` blocks withdrawals | `unstake` is never gated by `paused` (invariant #1) |
| Fake impact | Compromised oracle key | Rotatable oracle, off-chain multi-attestor, on-chain evidence hash |
| Governance capture | Flash-stake to vote | 7-day stake lock; voting via Realms with its own guards |
| Reinit attack | `init_if_needed` on position | Owner bound to seeds; fields set each call |
| Overflow | Large amounts | `checked_add/sub`, `overflow-checks=true` |
| Incident response | Live exploit | `paused` circuit breaker gates stake, impact registration and the marketplace — not the return of principal |
| Forged "verified" result on the check page | Crafted file name / markup injection | No untrusted input via innerHTML; strict CSP (`script-src 'self'`) |
| Register tampering | Register points to an anchor someone else paid for, or to a failed tx | Check page accepts only successful anchors paid by `ANCHOR_SIGNERS` |
| Double counting | Two parallel verifications of one serial | Ledger `claim` checks and records under an exclusive lock |
| Metadata tamper | Mutable metadata | Set `isMutable=false` at mainnet; revoke update authority |
| Supply inflation | Residual mint authority | Script revokes mint + freeze authority |

## Off-chain oracle hardening (design)
- Keys in an HSM / KMS; never in the repo or a hot server env var.
- Threshold signing (e.g. m-of-n) so no single machine can attest.
- Rate limits + anomaly alerts on `register_impact`.
- Deterministic, reproducible proof pipeline (evidence hash matches source).
