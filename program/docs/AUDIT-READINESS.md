# Audit-Readiness Package

Purpose: make an external audit faster, cheaper, and more thorough. This is
**not** a substitute for a test, independent audit — it is the material you hand
to the auditor.

## 1. What to send the auditor
- Frozen commit hash + `Cargo.lock` / `yarn.lock`.
- This repo including `docs/THREAT-MODEL.md` and the test suite.
- Deployed devnet program ID + IDL.
- Architecture note: which authorities exist, who holds them at launch, and the
  migration plan to DAO control.

## 2. Pre-audit self-checklist (Solana/Anchor)
- [ ] Signer checks on every privileged instruction (`has_one`, `address =`).
- [ ] Account ownership & type enforced by Anchor `Account<...>` (no raw
      `AccountInfo` where a typed account is expected).
- [ ] PDA seeds documented; bumps persisted; no `find_program_address` in hot
      paths.
- [ ] Checked arithmetic everywhere; `overflow-checks = true` in release.
- [ ] No `init_if_needed` reinitialisation attack surface (fields set every call
      or guarded).
- [ ] CPIs use `CpiContext` with correct signer seeds; no unchecked CPI targets.
- [ ] Token accounts validated for mint + owner before transfer.
- [ ] Rent-exemption sizes match `LEN` constants exactly.
- [ ] Circuit breaker (`paused`) gates all state-changing user paths.
- [ ] Events emitted for every state change (indexer + forensic trail).
- [ ] No secrets/keys in the repo; oracle key management documented.

## 3. Recommended audit firms (Solana-specialised)
OtterSec, Neodyme, Halborn, Zellic, Sec3, Kudelski. Get 2–3 quotes; scope by
program size and instruction count. Typical range for a program this size:
USD 8,000–30,000 and 1–3 weeks.

## 4. Tooling to run before the audit
- `anchor test` (all green), plus `solana-test-validator` fuzzing.
- `cargo clippy -- -D warnings`.
- `cargo-audit` for dependency CVEs.
- Optional: `trident` (Solana fuzzing) or `honggfuzz` on core math.

## 5. After the audit
- Fix all Critical/High before mainnet; document Medium/Low decisions.
- Publish the report (or a summary) for user trust.
- Re-audit if the code changes materially after sign-off.
