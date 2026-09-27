# Security Policy

## Reporting
Email kontakt@terrasols.org (subject "Sicherheit") with a description,
reproduction, and impact. Please allow 90 days for coordinated disclosure.
Do **not** open public issues for vulnerabilities.

## Scope
- On-chain program `programs/terrasol`
- Token creation & authority-revocation script
- Off-chain oracle signing service (design in `program/docs/THREAT-MODEL.md`)
- Public check page and its API (`oracle/verify-server.ts`, `oracle/public/`)

## Design invariants (must always hold)
1. Staking never mints, accrues, or pays any reward. Principal in == principal
   out, and the return of principal after the lock is never blocked — not
   even by `paused`. (Legal: preserves utility-token status.)
2. Only the configured `oracle` can write impact records.
3. Only the `governance` authority can change config, rotate the oracle, pause,
   or transfer governance.
4. TRRA supply is fixed (mint authority revoked) and non-freezable.
5. All arithmetic is checked; no path may overflow silently.
6. The vault can only be debited by the program PDA under the documented seeds
   (the config PDA, which is the vault's token authority).
7. Only the program's upgrade authority can initialize the config.
8. Governance changes hands only when the new key accepts.

Any change that weakens invariant #1 requires a fresh legal review before merge.
