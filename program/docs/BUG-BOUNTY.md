# Bug Bounty (template — set live before mainnet)

## Platform
List on Immunefi (or similar) once audited. Keep a self-hosted fallback at
hello@terrasols.org.

## Severity & reward guide (USD, illustrative)
| Severity | Example | Reward range |
|----------|---------|--------------|
| Critical | Vault drain, arbitrary mint, governance takeover | 10,000–50,000 |
| High     | Freeze funds, forge impact at scale | 5,000–10,000 |
| Medium   | Griefing, DoS on an instruction | 1,000–5,000 |
| Low      | Info leak, minor spec deviation | 250–1,000 |

## Rules
- No testing on mainnet with real user funds; use devnet or a fork.
- No social engineering, no DoS against RPC infrastructure.
- First reporter of a unique, reproducible bug is eligible.
- Coordinated disclosure; rewards scale with impact and quality of report.
