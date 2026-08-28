// Legt einen echten Nachweis an und schreibt das zugehoerige Dokument als
// Datei - damit laesst sich die Pruefseite im Browser mit einem echten Upload
// testen. Aufruf: npx ts-node demo-daten.ts
process.env.GS_MODE = "snapshot";
process.env.GS_SNAPSHOT = "./data/gold-standard-retirements.json";
process.env.LEDGER_PATH = "./data/demo-ledger.json";
process.env.PROOF_STORE = process.env.PROOF_STORE ?? "./data/proofs.json";

import fs from "fs";
import { Connection, Keypair, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { verifyEvidence, evidenceHash } from "./verify";
import { anchorProof } from "./chain-memo";
import { proofStore } from "./proof-store";

const RPC = process.env.RPC_URL ?? "http://127.0.0.1:8899";

async function main() {
  const r = await verifyEvidence({
    subject: "11111111111111111111111111111111",
    co2eTonnes: 800,
    registry: { standard: "gold_standard", serial: "GS1-1-BR-GS1368-2021-1000" },
  });
  if (!r.ok) throw new Error("Verifizierung fehlgeschlagen: " + r.reason);

  const hash = evidenceHash(r.evidence);
  const hashHex = Buffer.from(hash).toString("hex");

  const conn = new Connection(RPC, "confirmed");
  const zahler = Keypair.generate();
  const s = await conn.requestAirdrop(zahler.publicKey, 2 * LAMPORTS_PER_SOL);
  await conn.confirmTransaction(s, "confirmed");

  const a = await anchorProof(conn, zahler, hash);
  proofStore.add({ hashHex, signature: a.signature, blockTime: null, note: "Demo" });

  // Das Dokument selbst - genau diese Bytes wurden verankert.
  fs.writeFileSync("./data/nachweis-echt.bin", r.evidence);
  const veraendert = Buffer.from(r.evidence);
  veraendert[0] ^= 0x01;
  fs.writeFileSync("./data/nachweis-veraendert.bin", veraendert);

  console.log("Hash:      " + hashHex);
  console.log("Signatur:  " + a.signature);
  console.log("Dateien:   data/nachweis-echt.bin, data/nachweis-veraendert.bin");
}
main().catch(e => { console.error(e); process.exit(1); });
