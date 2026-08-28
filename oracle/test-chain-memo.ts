// Self-test for on-chain anchoring. Run: npx ts-node test-chain-memo.ts
//
// Runs against a LOCAL Solana node — a real runtime, unlimited play money, no
// public faucet in the way:
//
//   solana-test-validator --reset --quiet
//
// What this proves, in order of what actually matters:
//   1. the payload format survives a round trip
//   2. a proof can be written to a real chain and read back
//   3. a document that was altered by ONE byte no longer matches
//
// Point 3 is the whole product. Without it the other two are bookkeeping.

process.env.GS_MODE = "snapshot";
process.env.GS_SNAPSHOT = "./data/gold-standard-retirements.json";
process.env.LEDGER_PATH = "./data/_test-anchor-ledger.json";

import fs from "fs";
import { Connection, Keypair, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { verifyEvidence, evidenceHash } from "./verify";
import {
  anchorProof,
  costPerProof,
  memoPayload,
  parseMemo,
  readProof,
  verifyProof,
} from "./chain-memo";

const RPC = process.env.RPC_URL ?? "http://127.0.0.1:8899";
const SUBJECT = "11111111111111111111111111111111";

let failures = 0;
function check(name: string, cond: boolean, detail = "") {
  console.log(`${cond ? "PASS" : "FAIL"}  ${name}${detail ? "   " + detail : ""}`);
  if (!cond) failures++;
}

async function fund(connection: Connection, who: Keypair): Promise<number> {
  const sig = await connection.requestAirdrop(who.publicKey, 2 * LAMPORTS_PER_SOL);
  await connection.confirmTransaction(sig, "confirmed");
  // Ask at "confirmed", not the default "finalized" — the latter lags roughly
  // 32 slots and would still report zero here.
  return connection.getBalance(who.publicKey, "confirmed");
}

async function main() {
  try { fs.unlinkSync(process.env.LEDGER_PATH!); } catch {}

  // -- 1. Payload format, no chain involved ------------------------------
  const dummy = Array.from({ length: 32 }, (_, i) => i);
  const payload = memoPayload(dummy);
  check("payload round trip", parseMemo(payload) === Buffer.from(dummy).toString("hex"));
  check("foreign memo ignored", parseMemo("hello world") === null);
  check(
    "wrong hash length rejected",
    (() => { try { memoPayload([1, 2, 3]); return false; } catch { return true; } })()
  );

  // -- 2. A real proof from the real pipeline ----------------------------
  const result = await verifyEvidence({
    subject: SUBJECT,
    co2eTonnes: 800,
    registry: { standard: "gold_standard", serial: "GS1-1-BR-GS1368-2021-1000" },
  });
  check("verification pipeline delivers a proof", result.ok);
  const hash = evidenceHash(result.evidence);
  check("hash is 32 bytes", hash.length === 32);

  // -- 3. Write to the chain and read back -------------------------------
  const connection = new Connection(RPC, "confirmed");
  const payer = Keypair.generate();
  const balance = await fund(connection, payer);
  check("payer funded", balance > 0, `${(balance / LAMPORTS_PER_SOL).toFixed(2)} SOL`);

  const before = await connection.getBalance(payer.publicKey, "confirmed");
  const anchored = await anchorProof(connection, payer, hash);
  const after = await connection.getBalance(payer.publicKey, "confirmed");
  console.log(`\n      Signatur: ${anchored.signature}`);
  console.log(`      Memo:     ${anchored.memo}\n`);

  const read = await readProof(connection, anchored.signature);
  check("proof readable from the chain", read.hashHex !== null);
  check("hash on chain matches the document", read.hashHex === anchored.hashHex);
  check("chain carries a timestamp", read.blockTime !== null,
        read.blockTime ? new Date(read.blockTime * 1000).toISOString() : "");

  const v = await verifyProof(connection, anchored.signature, hash);
  check("verifyProof confirms the match", v.match);

  // -- 4. The point of the whole exercise --------------------------------
  // One byte changed in the document. Nothing else.
  const tampered = Buffer.from(result.evidence);
  tampered[0] = tampered[0] ^ 0x01;
  const tamperedHash = evidenceHash(tampered);
  const vt = await verifyProof(connection, anchored.signature, tamperedHash);
  check("altered document no longer matches", !vt.match);

  // Only the hash is on the chain — no serial, no customer, no project id.
  check(
    "no personal data on the chain",
    !anchored.memo.includes("GS1-1-BR") && !anchored.memo.includes(SUBJECT)
  );

  // -- 5. Cost -----------------------------------------------------------
  const paid = before - after;
  const cost = costPerProof(95.09);
  console.log(
    `\n      Kosten je Nachweis: ${paid} Lamports gezahlt, ` +
    `${cost.lamports} erwartet = ${cost.usd.toFixed(6)} USD bei SOL 95.09`
  );
  check("cost is one flat transaction fee", paid === cost.lamports);

  console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILED`);
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((e) => { console.error(e); process.exit(1); });
