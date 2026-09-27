// server/oracle/ledger.ts
//
// Append-only ledger of registry serials that already back an on-chain proof.
// This is the anti-double-counting anchor: one serial -> at most one Proof-of-Impact.
// For the pilot this is a JSON file; swap for a DB table (UNIQUE on (standard,serial))
// when you harden the service.
//
// Race-free by construction: `claim` checks AND records under an exclusive
// lock file, so two requests for the same serial can never both pass. Use it
// BEFORE anchoring; call `release` if anchoring fails, `commit` once it worked.
// Writes go to a temp file and are renamed into place, so a crash mid-write
// never leaves a truncated ledger.

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

function ledgerPath(): string {
  return process.env.LEDGER_PATH ?? "./data/used-serials.json";
}

function load(): Entry[] {
  try {
    return JSON.parse(fs.readFileSync(ledgerPath(), "utf-8"));
  } catch (e: any) {
    if (e?.code === "ENOENT") return [];
    // A corrupt ledger must stop the service, not silently start empty —
    // an empty ledger would let every serial be counted again.
    throw new Error(`ledger unreadable at ${ledgerPath()}: ${e?.message ?? e}`);
  }
}

function save(rows: Entry[]): void {
  const ziel = ledgerPath();
  fs.mkdirSync(path.dirname(ziel), { recursive: true });
  const tmp = `${ziel}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(rows, null, 2));
  fs.renameSync(tmp, ziel);
}

/** Runs `fn` while holding an exclusive lock file (works across processes). */
function locked<T>(fn: () => T): T {
  const lock = `${ledgerPath()}.lock`;
  fs.mkdirSync(path.dirname(lock), { recursive: true });
  const deadline = Date.now() + 5_000;
  let fd: number | null = null;
  while (fd === null) {
    try {
      fd = fs.openSync(lock, "wx");
    } catch (e: any) {
      if (e?.code !== "EEXIST") throw e;
      // A lock older than 30 s belongs to a crashed process.
      try {
        if (Date.now() - fs.statSync(lock).mtimeMs > 30_000) fs.unlinkSync(lock);
      } catch {}
      if (Date.now() > deadline) throw new Error("ledger lock timeout");
      const until = Date.now() + 10;
      while (Date.now() < until) {} // short spin; the pilot has little contention
    }
  }
  try {
    return fn();
  } finally {
    fs.closeSync(fd);
    try { fs.unlinkSync(lock); } catch {}
  }
}

function key(standard: Standard, serial: string): string {
  return `${standard}:${serial}`;
}

export const ledger = {
  /** True if this serial already backs (or is reserved for) a proof. */
  has(standard: Standard, serial: string): boolean {
    const k = key(standard, serial);
    return load().some((e) => key(e.standard, e.serial) === k);
  },

  /**
   * Atomically reserve a serial. Returns false if it is already taken.
   * Call BEFORE anchoring on-chain.
   */
  claim(standard: Standard, serial: string, subject: string): boolean {
    return locked(() => {
      const rows = load();
      const k = key(standard, serial);
      if (rows.some((e) => key(e.standard, e.serial) === k)) return false;
      rows.push({ standard, serial, subject, at: new Date().toISOString() });
      save(rows);
      return true;
    });
  },

  /** Undo a reservation whose anchoring failed (only if no txSig recorded). */
  release(standard: Standard, serial: string): void {
    locked(() => {
      const k = key(standard, serial);
      const rows = load().filter((e) => key(e.standard, e.serial) !== k || e.txSig);
      save(rows);
    });
  },

  /**
   * Record a serial as used, with the tx signature once register_impact (or
   * the memo anchor) succeeded. Idempotent; completes an earlier `claim`.
   */
  commit(standard: Standard, serial: string, subject: string, txSig?: string): void {
    locked(() => {
      const rows = load();
      const k = key(standard, serial);
      const vorhanden = rows.find((e) => key(e.standard, e.serial) === k);
      if (vorhanden) {
        if (txSig && !vorhanden.txSig) {
          vorhanden.txSig = txSig;
          save(rows);
        }
        return;
      }
      rows.push({ standard, serial, subject, txSig, at: new Date().toISOString() });
      save(rows);
    });
  },

  all(): Entry[] {
    return load();
  },
};
