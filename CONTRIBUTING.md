# Contributing

TerraSol is being opened up as a public good on Solana (a requirement of the
grant programmes we apply to). Until the first external audit is complete,
changes to `program/` are restricted to the core team; issues and pull
requests against `engine/`, `oracle/` and `token/` are welcome.

Ground rules:

- Every change ships with a test that fails without it.
- Nothing lands that has not been run against a real local validator
  (`bash program/alles-testen.sh`).
- No yield, profit or price language anywhere. TRRA is a utility token;
  wording is part of the legal design, not marketing copy.

See `SECURITY.md` for how to report vulnerabilities.
