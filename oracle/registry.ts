// server/oracle/registry.ts
//
// Registry abstraction for TerraSol impact verification.
// Multi-registry from day one; Gold Standard implemented, Verra stubbed.
//
// The Gold Standard "GSF Registry" (registry.goldstandard.org) is a web registry.
// At the time of writing there is no officially documented open JSON API, so two
// modes are supported:
//   GS_MODE=snapshot  -> verify against a local export the operator downloads from
//                        the registry's Retirements / credit-blocks view. Fully
//                        deterministic, works offline. RECOMMENDED for the first pilot.
//   GS_MODE=live      -> query a JSON endpoint. Base URL + path are configurable and
//                        MUST be confirmed against the current GSF Registry API /
//                        user guide before you rely on them.

import fs from "fs";

export type Standard = "gold_standard" | "verra";

export interface RegistryRef {
  standard: Standard;
  serial: string;          // credit block / serial (range) — the unique identifier
  projectId?: string;      // e.g. "GS1368"
  vintage?: number;        // issuance year
  quantityTonnes?: number; // claimed amount (validated against the registry record)
}

export interface RegistryRecord {
  standard: Standard;
  serial: string;
  projectId?: string;
  vintage?: number;
  quantityTonnes: number;  // authoritative amount from the registry
  retired: boolean;        // retired = permanently claimed, cannot be resold
  retiredAt?: string;
  beneficiary?: string;    // who the retirement was made for, if published
  raw?: unknown;           // original row, for audit
}

export interface Registry {
  lookup(ref: RegistryRef): Promise<RegistryRecord | null>;
}

// Node 18+ has a global fetch. Cast to avoid DOM lib type noise.
const httpFetch: (url: string, init?: any) => Promise<any> = (globalThis as any).fetch;

// ---- Gold Standard ----------------------------------------------------------
class GoldStandardRegistry implements Registry {
  async lookup(ref: RegistryRef): Promise<RegistryRecord | null> {
    const mode = process.env.GS_MODE ?? "snapshot";
    return mode === "live" ? this.lookupLive(ref) : this.lookupSnapshot(ref);
  }

  // Deterministic, offline. Point GS_SNAPSHOT at a JSON export of retirements.
  private lookupSnapshot(ref: RegistryRef): RegistryRecord | null {
    const path = process.env.GS_SNAPSHOT ?? "./data/gold-standard-retirements.json";
    if (!fs.existsSync(path)) {
      throw new Error(`GS snapshot not found at ${path} (set GS_SNAPSHOT)`);
    }
    const rows: any[] = JSON.parse(fs.readFileSync(path, "utf-8"));
    const row = rows.find((r) => String(r.serial ?? r.serialNumber) === ref.serial);
    if (!row) return null;
    return normalise(ref, row);
  }

  // Live JSON endpoint. CONFIRM the path/shape against the current GSF Registry API.
  private async lookupLive(ref: RegistryRef): Promise<RegistryRecord | null> {
    const base = process.env.GS_API_BASE ?? "https://registry.goldstandard.org";
    const url = `${base}/api/credit-blocks?serial=${encodeURIComponent(ref.serial)}`;
    const res = await httpFetch(url, { headers: gsHeaders() });
    if (!res.ok) throw new Error(`GS registry HTTP ${res.status}`);
    const data: any = await res.json();
    const row = Array.isArray(data) ? data[0] : data.results?.[0] ?? data;
    if (!row) return null;
    return normalise(ref, row);
  }
}

function normalise(ref: RegistryRef, row: any): RegistryRecord {
  const status = String(row.status ?? row.state ?? "");
  return {
    standard: "gold_standard",
    serial: ref.serial,
    projectId: row.projectId ?? row.project ?? row.project_id ?? ref.projectId,
    vintage: row.vintage != null ? Number(row.vintage) : ref.vintage,
    quantityTonnes: Number(row.quantity ?? row.quantityTonnes ?? row.volume ?? 0),
    retired: row.retired === true || /retir/i.test(status),
    retiredAt: row.retiredAt ?? row.retirementDate ?? row.retired_at,
    beneficiary: row.beneficiary ?? row.retiredFor ?? row.retired_for,
    raw: row,
  };
}

function gsHeaders(): Record<string, string> {
  const key = process.env.GS_API_KEY;
  return key
    ? { authorization: `Bearer ${key}`, accept: "application/json" }
    : { accept: "application/json" };
}

// ---- Verra (placeholder) ----------------------------------------------------
class VerraRegistry implements Registry {
  async lookup(_ref: RegistryRef): Promise<RegistryRecord | null> {
    throw new Error("Verra registry not implemented yet");
  }
}

const gold = new GoldStandardRegistry();
const verra = new VerraRegistry();

export function getRegistry(standard: Standard): Registry {
  return standard === "verra" ? verra : gold;
}
