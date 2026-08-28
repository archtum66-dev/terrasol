# Roadmap

Dates are targets, not promises. Every phase has a hard gate; nothing ships
to mainnet before its gate is passed. The one decision that shapes
everything: **one token.** TRRA serves both TerraSol (verification,
marketplace, staking tiers) and VERA (paying for object verification) — two
applications, one token, one legal dossier, undivided liquidity.

## Phase 0 — Components proven · done (Aug 2026)

Engine benchmarked (11.8M ops/s; 5.06M orders/s over TCP with bundles).
Smart contract compiled for the first time, deployed to a local validator,
15/15 end-to-end checks. Oracle pipeline + public check page 29/29 checks,
document never leaves the customer's machine. TRRA minted on a local chain
with metadata read back from the chain; authorities revoked. Settlement demo:
engine state anchored on-chain for one flat fee. Four real bugs found and
fixed by running, not reading.

## Phase 1 — First revenue, token-less · Sep–Oct 2026

Gate: a paying pilot customer.
Check page live on terrasols.org; verification billed per proof (manual
invoice — service revenue, not capital markets). Venture Kick application
**before** founding any entity; Solana Foundation grant sketch (this repo is
the public-good deliverable); Klimastiftung Schweiz. Lawyer reviews the
service terms (already drafted). No token sale — per the July 2026 decision,
a public token crowdfund is maximum disclosure, regulation and capital need
at once, and is deferred until traction.

## Phase 2 — Public devnet · Q4 2026

Gate: pilot revenue exists; grant answers pending.
Program deployed to public devnet under a fresh ID; oracle key moved to a
KMS; token metadata moved from terrasols.org to Arweave/IPFS (a domain can
lapse — an on-chain URI cannot be re-pointed after authorities are revoked).
Monitoring, structured logs, restore drills. Angel/pre-seed conversations as
**equity in the operating company — never the token** — which keeps TRRA's
utility classification intact.

## Phase 3 — Audit, then mainnet · Q1 2027

Gate (hard): external audit passed + bug bounty live — the project's own
standing rule.
Mainnet program deploy (~2.3 SOL rent measured). TRRA mainnet issuance
exactly as specified: 100M fixed, 9 decimals, mint+freeze revoked in the
issuance transaction, metadata on Arweave. Staking tiers 100/1k/10k/100k
active. If public tradability is intended, a FINMA no-action/subordination
request goes out **before** issuance, not after.

## Phase 4 — Marketplace at engine speed · Q2 2027

Gate: mainnet stable one quarter; legal green light for credit trading.
The Hyperliquid-pattern goes live: the Rust engine matches credit orders
off-chain (measured 5M orders/s with bundled signatures), Solana settles —
TRRA payment legs on-chain, engine state anchored per batch (measured ~0.01
lamports per fill). Registry consent (Verra/Gold Standard) is the gate for
tokenising credits themselves; until then the marketplace trades only
oracle-attested records, which the deployed program already supports.

## Phase 5 — Governance and second application · H2 2027

Realms/SPL-Governance takes over the config authority (multisig first, DAO
vote second). VERA integration: TRRA pays for physical-object verification —
the "usable on day one" property that anchors the utility classification.
EU distribution only behind a MiCA whitepaper; US persons excluded until
counsel says otherwise.

## Standing rules

Audit before mainnet. No yield language, ever. Hash on chain, data off
chain. Equity finances the company; the token never does. Every claim in
this repo is measured or it is deleted.
