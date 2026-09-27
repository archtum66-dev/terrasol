// Hardening tests for the public check service. Run: npx ts-node test-haertung.ts
//
// No chain required — runs in CI on every push. Each check pins down one
// weakness found in the September 2026 review, so it cannot come back:
//   - one request with a malformed Host header crashed the server
//   - the page put the file name into innerHTML (forged "verified" result)
//   - /api/status revealed the RPC URL (API keys live there)
//   - a failed or foreign-signed anchor transaction counted as proof
//   - two parallel verifications of one serial could both pass

import os from "os";
import path from "path";
const TMP = path.join(os.tmpdir(), `terrasol-haertung-${process.pid}`);
process.env.PROOF_STORE = path.join(TMP, "proofs.json");
process.env.LEDGER_PATH = path.join(TMP, "ledger.json");
process.env.RPC_URL = "https://rpc.example.invalid/?api-key=GEHEIM";
process.env.RATE_LIMIT_PER_MIN = "5";

import fs from "fs";
import net from "net";
import http from "http";
import { AddressInfo } from "net";
import { Keypair, PublicKey, TransactionMessage } from "@solana/web3.js";

import { server, zuViele } from "./verify-server";
import { anchorInstruction, readProof, verifyProof } from "./chain-memo";
import { ledger } from "./ledger";

let failures = 0;
function check(name: string, cond: boolean, detail = "") {
  console.log(`${cond ? "PASS" : "FAIL"}  ${name}${detail ? "   " + detail : ""}`);
  if (!cond) failures++;
}

function roh(port: number, anfrage: string): Promise<string> {
  return new Promise((resolve) => {
    const s = net.connect(port, "127.0.0.1", () => s.write(anfrage));
    let daten = "";
    s.on("data", (d) => (daten += d.toString()));
    s.on("end", () => resolve(daten));
    s.on("error", () => resolve(daten));
    setTimeout(() => { s.destroy(); resolve(daten); }, 3000);
  });
}

function holen(port: number, pfad: string, method = "GET"): Promise<{ code: number; kopf: http.IncomingHttpHeaders; body: string }> {
  return new Promise((resolve, reject) => {
    const r = http.request({ host: "127.0.0.1", port, path: pfad, method }, (res) => {
      let body = "";
      res.on("data", (d) => (body += d));
      res.on("end", () => resolve({ code: res.statusCode ?? 0, kopf: res.headers, body }));
    });
    r.on("error", reject);
    r.end();
  });
}

/** A fake RPC answer built from a real, compiled memo transaction. */
function fakeConnection(payer: PublicKey, hash: Buffer, err: unknown) {
  const message = new TransactionMessage({
    payerKey: payer,
    recentBlockhash: PublicKey.default.toBase58(),
    instructions: [anchorInstruction(payer, hash)],
  }).compileToV0Message();
  return {
    getTransaction: async () => ({
      slot: 42,
      blockTime: 1_700_000_000,
      transaction: { message, signatures: [] },
      meta: { err, loadedAddresses: { writable: [], readonly: [] }, logMessages: [] },
    }),
  } as any;
}

async function main() {
  fs.mkdirSync(TMP, { recursive: true });

  // -- 1. HTTP-Schicht ----------------------------------------------------
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as AddressInfo).port;

  const antwort = await roh(port, "GET /api/status HTTP/1.1\r\nHost: [\r\nConnection: close\r\n\r\n");
  check("malformed Host header answered", antwort.startsWith("HTTP/1.1"), antwort.split("\r\n")[0]);
  const danach = await holen(port, "/");
  check("server still alive afterwards", danach.code === 200);

  const csp = String(danach.kopf["content-security-policy"] ?? "");
  check("CSP allows only own scripts", csp.includes("script-src 'self'") && !/script-src[^;]*unsafe-inline/.test(csp));
  check("nosniff + frame protection", danach.kopf["x-content-type-options"] === "nosniff" && danach.kopf["x-frame-options"] === "DENY");
  check("page has no inline script", !danach.body.includes("<script>") && danach.body.includes('src="/pruefen.js"'));
  check("page has no inline event handler", !/\son[a-z]+=/i.test(danach.body));

  const js = await holen(port, "/pruefen.js");
  check("script served as JavaScript", String(js.kopf["content-type"]).startsWith("text/javascript"));
  check("script never uses innerHTML", js.code === 200 && !js.body.includes("innerHTML"));

  const post = await holen(port, "/api/pruefen", "POST");
  check("only GET/HEAD allowed", post.code === 405);

  const status = await holen(port, "/api/status");
  check("status does not reveal the RPC URL", !status.body.includes("GEHEIM") && !status.body.includes("example.invalid"), status.body);

  const trav = await holen(port, "/..%2f..%2fpackage.json");
  check("no path traversal", trav.code === 404);

  server.close();

  // -- 2. Ratenbegrenzung --------------------------------------------------
  const t = Date.now();
  const ergebnisse = Array.from({ length: 7 }, () => zuViele("10.0.0.9", t));
  check("rate limit kicks in after 5/min", ergebnisse.slice(0, 5).every((x) => !x) && ergebnisse[5] && ergebnisse[6]);
  check("rate limit resets after a minute", !zuViele("10.0.0.9", t + 61_000));

  // -- 3. Kette: nur erfolgreiche, eigene Verankerungen zählen -------------
  const hash = Buffer.alloc(32, 7);
  const wir = Keypair.generate().publicKey;
  const ok = await readProof(fakeConnection(wir, hash, null), "sig");
  check("memo read from the instruction", ok.hashHex === hash.toString("hex"));
  check("fee payer reported", ok.feePayer === wir.toBase58());
  check("successful tx not flagged", !ok.failed);

  const gescheitert = await verifyProof(fakeConnection(wir, hash, { InstructionError: [0, "Custom"] }), "sig", hash);
  check("failed transaction is no proof", !gescheitert.match);

  const fremd = await verifyProof(fakeConnection(Keypair.generate().publicKey, hash, null), "sig", hash, [wir.toBase58()]);
  check("anchor by a foreign key is no proof", !fremd.match);
  const eigen = await verifyProof(fakeConnection(wir, hash, null), "sig", hash, [wir.toBase58()]);
  check("anchor by our key is a proof", eigen.match);

  // -- 4. Doppelzählung: reservieren ist atomar ----------------------------
  const serial = "GS1-TEST-1";
  check("first claim wins", ledger.claim("gold_standard", serial, "A"));
  check("second claim loses", !ledger.claim("gold_standard", serial, "B"));
  ledger.release("gold_standard", serial);
  check("released serial can be claimed again", ledger.claim("gold_standard", serial, "B"));
  ledger.commit("gold_standard", serial, "B", "txsig");
  ledger.release("gold_standard", serial);
  check("committed serial survives release", ledger.has("gold_standard", serial));

  fs.writeFileSync(process.env.LEDGER_PATH!, "{kaputt");
  let wirft = false;
  try { ledger.has("gold_standard", "x"); } catch { wirft = true; }
  check("corrupt ledger stops instead of starting empty", wirft);

  fs.rmSync(TMP, { recursive: true, force: true });
  console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILED`);
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((e) => { console.error(e); process.exit(1); });
