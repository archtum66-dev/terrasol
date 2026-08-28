// Self-test for the check page. Run: npx ts-node test-pruefseite.ts
//
// Needs a local chain:  solana-test-validator --reset --quiet
//
// Walks the whole customer path once, for real:
//   verify a credit -> anchor the proof on-chain -> record it ->
//   ask the endpoint about the document -> ask it about a tampered document ->
//   ask it about a document that was never registered.

process.env.GS_MODE = "snapshot";
process.env.GS_SNAPSHOT = "./data/gold-standard-retirements.json";
process.env.LEDGER_PATH = "./data/_test-page-ledger.json";
process.env.PROOF_STORE = "./data/_test-page-proofs.json";

import fs from "fs";
import { createHash } from "crypto";
import { Connection, Keypair, LAMPORTS_PER_SOL } from "@solana/web3.js";

import { verifyEvidence, evidenceHash } from "./verify";
import { anchorProof } from "./chain-memo";
import { proofStore } from "./proof-store";
import { pruefen } from "./verify-server";

const RPC = process.env.RPC_URL ?? "http://127.0.0.1:8899";
const SUBJECT = "11111111111111111111111111111111";

let failures = 0;
function check(name: string, cond: boolean, detail = "") {
  console.log(`${cond ? "PASS" : "FAIL"}  ${name}${detail ? "   " + detail : ""}`);
  if (!cond) failures++;
}

const hex = (b: number[] | Buffer) => Buffer.from(b as Uint8Array).toString("hex");

async function main() {
  for (const p of [process.env.LEDGER_PATH!, process.env.PROOF_STORE!]) {
    try { fs.unlinkSync(p); } catch {}
  }

  // -- 1. Ein echter Nachweis entsteht ------------------------------------
  const ergebnis = await verifyEvidence({
    subject: SUBJECT,
    co2eTonnes: 800,
    registry: { standard: "gold_standard", serial: "GS1-1-BR-GS1368-2021-1000" },
  });
  check("Verifizierung liefert einen Nachweis", ergebnis.ok);

  const hash = evidenceHash(ergebnis.evidence);
  const hashHex = hex(hash);

  // -- 2. Auf die Kette und ins Register ----------------------------------
  const connection = new Connection(RPC, "confirmed");
  const zahler = Keypair.generate();
  const sig = await connection.requestAirdrop(zahler.publicKey, 2 * LAMPORTS_PER_SOL);
  await connection.confirmTransaction(sig, "confirmed");

  const verankert = await anchorProof(connection, zahler, hash);
  proofStore.add({
    hashHex,
    signature: verankert.signature,
    blockTime: null,
    note: "Selbsttest",
  });
  check("Nachweis im Register", proofStore.count() === 1, verankert.signature.slice(0, 20) + "…");

  // -- 3. Der Kunde legt das richtige Dokument vor ------------------------
  const gut: any = await pruefen(hashHex);
  check("richtiges Dokument -> registriert", gut.status === "registriert");
  check("Zeitstempel kommt von der Kette", typeof gut.blockTime === "number",
        gut.blockTime ? new Date(gut.blockTime * 1000).toISOString() : "");
  check("Signatur wird mitgeliefert", gut.signature === verankert.signature);

  // -- 4. Ein Byte geändert -----------------------------------------------
  const veraendert = Buffer.from(ergebnis.evidence);
  veraendert[0] ^= 0x01;
  const schlecht: any = await pruefen(hex(evidenceHash(veraendert)));
  check("verändertes Dokument -> nicht registriert", schlecht.status === "unbekannt");

  // -- 5. Ein nie registriertes Dokument ----------------------------------
  const fremd = createHash("sha256").update("irgendein anderes Dokument").digest("hex");
  const unbekannt: any = await pruefen(fremd);
  check("fremdes Dokument -> nicht registriert", unbekannt.status === "unbekannt");

  // -- 6. Unsinn wird abgewiesen ------------------------------------------
  const quatsch: any = await pruefen("kein hash");
  check("ungültige Eingabe abgewiesen", quatsch.status === "ungueltig");

  // -- 7. Register lügt, Kette entscheidet --------------------------------
  // Ein Eintrag, dessen Signatur zu einem ANDEREN Hash gehört: Das Register
  // behauptet etwas, die Kette widerspricht. Die Kette gewinnt.
  const fremderHash = createHash("sha256").update("untergeschoben").digest("hex");
  proofStore.add({ hashHex: fremderHash, signature: verankert.signature, blockTime: null });
  const manipuliert: any = await pruefen(fremderHash);
  check("falscher Registereintrag wird von der Kette widerlegt",
        manipuliert.status === "abweichung");

  console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILED`);
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((e) => { console.error(e); process.exit(1); });
