// Self-test for the verification pipeline. Run: npx ts-node test-verify.ts
// Uses the sample snapshot; writes its ledger to a temp file.
process.env.GS_MODE = "snapshot";
process.env.GS_SNAPSHOT = "./data/gold-standard-retirements.json";
process.env.LEDGER_PATH = "./data/_test-ledger.json";

import fs from "fs";
import { verifyEvidence, evidenceHash } from "./verify";
import { ledger } from "./ledger";

const SUBJECT = "11111111111111111111111111111111";
let failures = 0;
function check(name: string, cond: boolean) {
  console.log(`${cond ? "PASS" : "FAIL"}  ${name}`);
  if (!cond) failures++;
}

async function main() {
  try { fs.unlinkSync(process.env.LEDGER_PATH!); } catch {}

  // 1) Valid retired credit, partial claim -> ok
  const a = await verifyEvidence({
    subject: SUBJECT, co2eTonnes: 800,
    registry: { standard: "gold_standard", serial: "GS1-1-BR-GS1368-2021-1000" },
  });
  check("valid retired credit accepted", a.ok);
  check("grams = 800t * 1e6", a.co2eGrams === 800_000_000);

  // 2) Determinism: same input -> identical evidence hash
  const a2 = await verifyEvidence({
    subject: SUBJECT, co2eTonnes: 800,
    registry: { standard: "gold_standard", serial: "GS1-1-BR-GS1368-2021-1000" },
  });
  check("deterministic evidence hash",
    JSON.stringify(evidenceHash(a.evidence)) === JSON.stringify(evidenceHash(a2.evidence)));

  // 3) Over-claim beyond registry quantity -> reject
  const over = await verifyEvidence({
    subject: SUBJECT, co2eTonnes: 2000,
    registry: { standard: "gold_standard", serial: "GS1-1-BR-GS1368-2021-1000" },
  });
  check("over-claim rejected", !over.ok && /exceeds registry/.test(over.reason ?? ""));

  // 4) Not-retired credit -> reject
  const notRetired = await verifyEvidence({
    subject: SUBJECT, co2eTonnes: 100,
    registry: { standard: "gold_standard", serial: "GS1-1-IN-GS4521-2022-500" },
  });
  check("non-retired rejected", !notRetired.ok && /not retired/.test(notRetired.reason ?? ""));

  // 5) Unknown serial -> reject
  const unknown = await verifyEvidence({
    subject: SUBJECT, co2eTonnes: 1,
    registry: { standard: "gold_standard", serial: "DOES-NOT-EXIST" },
  });
  check("unknown serial rejected", !unknown.ok && /not found/.test(unknown.reason ?? ""));

  // 6) Anti-double-counting: commit the serial, then the same serial is rejected
  ledger.commit(a.record!.standard, a.record!.serial, SUBJECT, "FAKE_TX_SIG");
  const dup = await verifyEvidence({
    subject: SUBJECT, co2eTonnes: 100,
    registry: { standard: "gold_standard", serial: "GS1-1-BR-GS1368-2021-1000" },
  });
  check("double-count rejected after commit", !dup.ok && /double-count/.test(dup.reason ?? ""));

  try { fs.unlinkSync(process.env.LEDGER_PATH!); } catch {}
  console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILURE(S)`);
  process.exit(failures === 0 ? 0 : 1);
}
main();
