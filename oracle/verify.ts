// server/oracle/verify.ts
//
// Real verification pipeline for TerraSol impact proofs (replaces the demo stub).
// Deterministic: the same registry state + input always yields the same result and
// the same evidence hash. Keeps the exported names (verifyEvidence, evidenceHash)
// so chain.ts and server.ts need only minimal changes (see README_verify.md).

import { createHash } from "crypto";
import { getRegistry, RegistryRef, RegistryRecord, Standard } from "./registry";
import { ledger } from "./ledger";

/** SHA-256 of the canonical evidence bytes -> 32-byte array for on-chain storage. */
export function evidenceHash(buf: Buffer): number[] {
  return Array.from(createHash("sha256").update(buf).digest());
}

export interface VerifyInput {
  subject: string;                 // Solana pubkey the proof is attributed to
  evidenceUri?: string;            // optional; a GS… reference can be parsed from it
  co2eTonnes: number;              // claimed amount
  registry?: Partial<RegistryRef>; // preferred: { standard, serial, projectId, vintage }
}

export interface VerifyResult {
  ok: boolean;
  co2eGrams: number;
  reason?: string;
  evidence: Buffer;
  record?: RegistryRecord;         // present on success; used by the caller for ledger.commit
}

const TONNE_IN_GRAMS = 1_000_000;
// A valid impact proof requires a RETIRED credit (permanently claimed, not resellable).
// Override with REQUIRE_RETIRED=false only for explicit testing.
const REQUIRE_RETIRED = (process.env.REQUIRE_RETIRED ?? "true") !== "false";

function fail(reason: string): VerifyResult {
  return { ok: false, co2eGrams: 0, reason, evidence: Buffer.alloc(0) };
}

/** Build a RegistryRef from explicit fields, falling back to parsing the evidence URI. */
function resolveRef(input: VerifyInput): RegistryRef | null {
  const r = input.registry ?? {};
  const standard: Standard = (r.standard as Standard) ?? "gold_standard";
  let serial = r.serial;
  let projectId = r.projectId;
  if ((!serial || !projectId) && input.evidenceUri) {
    // Accept e.g. "gs://GS1368/GS1-1-BR-... " or "...GS1368-<serial>..."
    const proj = input.evidenceUri.match(/GS\d+/);
    if (proj && !projectId) projectId = proj[0];
    const ser = input.evidenceUri.match(/[A-Za-z0-9]{2,}-\d[\w-]*\d/);
    if (ser && !serial) serial = ser[0];
  }
  if (!serial) return null;
  return { standard, serial, projectId, vintage: r.vintage, quantityTonnes: r.quantityTonnes };
}

/** Stable key order -> deterministic bytes -> deterministic hash. */
function canonicalEvidence(rec: RegistryRecord, subject: string): Buffer {
  const obj = {
    standard: rec.standard,
    serial: rec.serial,
    projectId: rec.projectId ?? null,
    vintage: rec.vintage ?? null,
    quantityTonnes: rec.quantityTonnes,
    retired: rec.retired,
    retiredAt: rec.retiredAt ?? null,
    beneficiary: rec.beneficiary ?? null,
    subject,
  };
  return Buffer.from(JSON.stringify(obj));
}

export async function verifyEvidence(input: VerifyInput): Promise<VerifyResult> {
  if (!(input.co2eTonnes > 0)) return fail("co2eTonnes must be > 0");

  const ref = resolveRef(input);
  if (!ref) return fail("registry serial missing (provide registry.serial or a GS reference)");

  // 1) Look the credit up in the registry.
  let rec: RegistryRecord | null;
  try {
    rec = await getRegistry(ref.standard).lookup(ref);
  } catch (e: any) {
    return fail(`registry lookup failed: ${e?.message ?? e}`);
  }
  if (!rec) return fail(`credit not found in ${ref.standard} registry: ${ref.serial}`);

  // 2) Integrity: the credit must be retired (permanently claimed).
  if (REQUIRE_RETIRED && !rec.retired) {
    return fail("credit is not retired — impact not yet permanently claimed");
  }

  // 3) Quantity: the claim may not exceed the registry amount.
  if (rec.quantityTonnes + 1e-9 < input.co2eTonnes) {
    return fail(`claimed ${input.co2eTonnes} t exceeds registry quantity ${rec.quantityTonnes} t`);
  }

  // 4) Anti-double-counting: one serial backs at most one proof.
  if (ledger.has(rec.standard, rec.serial)) {
    return fail(`serial already verified (double-count): ${rec.serial}`);
  }

  // 5) Deterministic evidence + grams. Uses the CLAIMED tonnes (already <= registry),
  //    so a partial retirement can back a smaller proof.
  const evidence = canonicalEvidence(rec, input.subject);
  const co2eGrams = Math.round(input.co2eTonnes * TONNE_IN_GRAMS);

  return { ok: true, co2eGrams, evidence, record: rec };
}
