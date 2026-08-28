// server/oracle/ledger.ts
//
// Append-only ledger of registry serials that already back an on-chain proof.
// This is the anti-double-counting anchor: one serial -> at most one Proof-of-Impact.
// For the pilot this is a JSON file; swap for a DB table (UNIQUE on (standard,serial))
// when you harden the service.

import fs from "fs";
import path from "path";
import { Standard } from "./registry";

interface Entry {
  standard: Standard;
  serial: string;
  subject: string;
  txSig?: string;
  at: string;
}

const LEDGER_PATH = process.env.LEDGER_PATH ?? "./data/used-serials.json";

function load(): Entry[] {
  try {
    return JSON.parse(fs.readFileSync(LEDGER_PATH, "utf-8"));
  } catch {
    return [];
  }
}

function save(rows: Entry[]): void {
  fs.mkdirSync(path.dirname(LEDGER_PATH), { recursive: true });
  fs.writeFileSync(LEDGER_PATH, JSON.stringify(rows, null, 2));
}

function key(standard: Standard, serial: string): string {
  return `${standard}:${serial}`;
}

export const ledger = {
  /** True if this serial already backs a proof. Checked during verification. */
  has(standard: Standard, serial: string): boolean {
    const k = key(standard, serial);
    return load().some((e) => key(e.standard, e.serial) === k);
  },

  /**
   * Record a serial as used. Call this ONLY AFTER register_impact succeeds on-chain,
   * passing the tx signature. Idempotent: a serial is never written twice.
   */
  commit(standard: Standard, serial: string, subject: string, txSig?: string): void {
    const rows = load();
    const k = key(standard, serial);
    if (rows.some((e) => key(e.standard, e.serial) === k)) return;
    rows.push({ standard, serial, subject, txSig, at: new Date().toISOString() });
    save(rows);
  },

  all(): Entry[] {
    return load();
  },
};
