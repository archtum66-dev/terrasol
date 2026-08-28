// server/oracle/chain-memo.ts
//
// On-chain anchoring WITHOUT a custom program.
//
// Why this exists
// ---------------
// The Anchor program in 01_Code/terrasol gives structured on-chain state
// (staking, marketplace, governance). It is also the one thing that blocks the
// lean MVP: it has to be compiled, audited and deployed before a single proof
// can be anchored — roughly USD 130–330 on mainnet, plus the whole toolchain.
//
// A Proof-of-Impact does not need any of that. What a customer, an auditor or a
// court actually wants to check is one question: *did this exact document exist
// at that point in time, unchanged?* A SHA-256 hash written into a Solana
// transaction answers it — permanently, timestamped by the chain, and verifiable
// by anyone with a block explorer and no trust in us at all.
//
// Cost: one ordinary transaction fee, 5000 lamports. At SOL 95 that is about
// USD 0.0005 per proof. No program account, no rent, no deploy.
//
// This is the "on-chain anchor as an add-on" path from
// TerraSol_Lean_MVP_Tokenlos.docx, section 4.
//
// Privacy — the reason only the hash goes on-chain
// ------------------------------------------------
// Serial numbers, project ids and customer wallets are personal or commercial
// data. Written to a public chain they are there forever, for everyone, and
// cannot be deleted — which collides head-on with revDSG and GDPR.
// Therefore: only the hash goes on-chain. Everything else stays in the service
// database. Whoever holds the document can recompute the hash and check it
// against the chain; whoever does not, learns nothing.
//
// The Anchor program stays on the roadmap. It is not needed to earn the first
// franc.

import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";

/** Solana's SPL Memo program v2. Same address on mainnet and devnet. */
export const MEMO_PROGRAM_ID = new PublicKey(
  "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"
);

/** Version tag, so a later format change stays distinguishable. */
export const PREFIX = "TSOL1";

/** Flat transaction fee in lamports. */
export const FEE_LAMPORTS = 5000;

export interface AnchorResult {
  signature: string;
  memo: string;
  hashHex: string;
}

export interface ProofOnChain {
  hashHex: string | null;
  blockTime: number | null;
  slot: number;
  signature: string;
}

// ---------------------------------------------------------------------------
function toHex(hash: number[] | Buffer | Uint8Array): string {
  return Buffer.from(hash as Uint8Array).toString("hex");
}

/** The exact bytes that go on-chain. Nothing else. */
export function memoPayload(hash: number[] | Buffer | Uint8Array): string {
  const hex = toHex(hash);
  if (hex.length !== 64) {
    throw new Error(`expected a 32-byte SHA-256 hash, got ${hex.length / 2} bytes`);
  }
  return `${PREFIX} ${hex}`;
}

/** Reads a hash back out of a memo. Returns null if it is not one of ours. */
export function parseMemo(memo: string): string | null {
  const m = memo.trim().match(/^TSOL1\s+([0-9a-f]{64})$/i);
  return m ? m[1].toLowerCase() : null;
}

/** The instruction on its own — useful for batching or for simulation. */
export function anchorInstruction(
  payer: PublicKey,
  hash: number[] | Buffer | Uint8Array
): TransactionInstruction {
  return new TransactionInstruction({
    keys: [{ pubkey: payer, isSigner: true, isWritable: false }],
    programId: MEMO_PROGRAM_ID,
    data: Buffer.from(memoPayload(hash), "utf8"),
  });
}

/**
 * Writes the hash on-chain. Returns the signature — that signature IS the
 * proof and belongs in the ledger next to the serial.
 */
export async function anchorProof(
  connection: Connection,
  payer: Keypair,
  hash: number[] | Buffer | Uint8Array
): Promise<AnchorResult> {
  const tx = new Transaction().add(anchorInstruction(payer.publicKey, hash));
  const signature = await sendAndConfirmTransaction(connection, tx, [payer], {
    commitment: "confirmed",
  });
  return { signature, memo: memoPayload(hash), hashHex: toHex(hash) };
}

/** Reads a proof back off the chain. No trust in our own database required. */
export async function readProof(
  connection: Connection,
  signature: string
): Promise<ProofOnChain> {
  const tx = await connection.getTransaction(signature, {
    commitment: "confirmed",
    maxSupportedTransactionVersion: 0,
  });
  if (!tx) {
    throw new Error(`transaction not found: ${signature}`);
  }

  // The memo shows up in the log as: Program log: Memo (len N): "…"
  let hashHex: string | null = null;
  for (const line of tx.meta?.logMessages ?? []) {
    const quoted = line.match(/Memo \(len \d+\): "(.*)"$/);
    if (quoted) {
      hashHex = parseMemo(quoted[1]);
      if (hashHex) break;
    }
  }

  // Fallback: read the instruction data directly. Works even when logs are
  // truncated, and is the more honest source — logs are convenience, the
  // instruction is the fact.
  if (!hashHex) {
    const msg = tx.transaction.message;
    const keys = msg.getAccountKeys({
      accountKeysFromLookups: tx.meta?.loadedAddresses,
    });
    for (const ix of msg.compiledInstructions) {
      const programId = keys.get(ix.programIdIndex);
      if (programId?.equals(MEMO_PROGRAM_ID)) {
        hashHex = parseMemo(Buffer.from(ix.data).toString("utf8"));
        if (hashHex) break;
      }
    }
  }

  return { hashHex, blockTime: tx.blockTime ?? null, slot: tx.slot, signature };
}

/**
 * The question a customer or an auditor actually asks:
 * does this document match what was anchored, and when was that?
 */
export async function verifyProof(
  connection: Connection,
  signature: string,
  expectedHash: number[] | Buffer | Uint8Array
): Promise<{ match: boolean; anchoredAt: Date | null; onChain: ProofOnChain }> {
  const onChain = await readProof(connection, signature);
  const expected = toHex(expectedHash);
  return {
    match: onChain.hashHex === expected,
    anchoredAt: onChain.blockTime ? new Date(onChain.blockTime * 1000) : null,
    onChain,
  };
}

/** What one proof costs. For the price list — not an estimate, a fixed fee. */
export function costPerProof(solPriceUsd: number): {
  lamports: number;
  sol: number;
  usd: number;
} {
  const sol = FEE_LAMPORTS / 1e9;
  return { lamports: FEE_LAMPORTS, sol, usd: sol * solPriceUsd };
}
