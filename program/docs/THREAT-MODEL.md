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
| Vault drain | Forged unstake / bad CPI signer | PDA-signed transfers, owner+mint checks, checked math |
| Fake impact | Compromised oracle key | Rotatable oracle, off-chain multi-attestor, on-chain evidence hash |
| Governance capture | Flash-stake to vote | 7-day stake lock; voting via Realms with its own guards |
| Reinit attack | `init_if_needed` on position | Owner bound to seeds; fields set each call |
| Overflow | Large amounts | `checked_add/sub`, `overflow-checks=true` |
| Incident response | Live exploit | `paused` circuit breaker gates all user paths |
| Metadata tamper | Mutable metadata | Set `isMutable=false` at mainnet; revoke update authority |
| Supply inflation | Residual mint authority | Script revokes mint + freeze authority |

## Off-chain oracle hardening (design)
- Keys in an HSM / KMS; never in the repo or a hot server env var.
- Threshold signing (e.g. m-of-n) so no single machine can attest.
- Rate limits + anomaly alerts on `register_impact`.
- Deterministic, reproducible proof pipeline (evidence hash matches source).
