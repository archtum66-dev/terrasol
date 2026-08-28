// server/oracle/proof-store.ts
//
// Register: document hash -> on-chain signature.
//
// The ledger in ledger.ts answers "has this registry serial already been
// used?". This store answers the customer's question instead: "here is my
// document — is it registered, and since when?"
//
// Deliberately keyed by the HASH, not by a customer id or a serial. A lookup
// therefore reveals nothing about who registered what: whoever does not
// already hold the document cannot ask a meaningful question.
//
// For the pilot a JSON file. For production a table with UNIQUE(hash).

import fs from "fs";
import path from "path";

export interface ProofEntry {
  hashHex: string;
  signature: string;
  blockTime: number | null;
  /** Free-form label for our own records. Never leaves the server. */
  note?: string;
  at: string;
}

const STORE_PATH = process.env.PROOF_STORE ?? "./data/proofs.json";

function load(): ProofEntry[] {
  try {
    return JSON.parse(fs.readFileSync(STORE_PATH, "utf-8"));
  } catch {
    return [];
  }
}

function save(rows: ProofEntry[]): void {
  fs.mkdirSync(path.dirname(STORE_PATH), { recursive: true });
  fs.writeFileSync(STORE_PATH, JSON.stringify(rows, null, 2));
}

export const proofStore = {
  /** Called after anchorProof succeeded. Idempotent. */
  add(entry: Omit<ProofEntry, "at">): ProofEntry {
    const rows = load();
    const existing = rows.find((r) => r.hashHex === entry.hashHex);
    if (existing) return existing;
    const row: ProofEntry = { ...entry, at: new Date().toISOString() };
    rows.push(row);
    save(rows);
    return row;
  },

  find(hashHex: string): ProofEntry | undefined {
    return load().find((r) => r.hashHex === hashHex.toLowerCase());
  },

  all(): ProofEntry[] {
    return load();
  },

  count(): number {
    return load().length;
  },
};
