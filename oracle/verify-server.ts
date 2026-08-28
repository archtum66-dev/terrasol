// server/oracle/verify-server.ts
//
// The public check page plus the one endpoint behind it.
//
//   GET  /                      the page
//   GET  /api/pruefen?hash=...  is this hash anchored, and since when?
//   GET  /api/status            how many proofs, which chain
//
// Node's built-in http server — no Express, no framework. The whole service is
// one lookup and one chain query; a framework would be more moving parts than
// product.
//
// The document is hashed IN THE BROWSER. Only 64 hex characters reach this
// server. That is not a nicety: it means a customer can check a confidential
// document without giving it to us, and it means our logs cannot leak
// documents we never received.
//
// Start:  npx ts-node verify-server.ts        (RPC_URL, PORT via env)

import fs from "fs";
import http from "http";
import path from "path";
import { Connection } from "@solana/web3.js";

import { proofStore } from "./proof-store";
import { readProof } from "./chain-memo";

const PORT = Number(process.env.PORT ?? 8080);
const RPC = process.env.RPC_URL ?? "http://127.0.0.1:8899";
const PUBLIC_DIR = path.join(__dirname, "public");

/** Solscan cluster suffix, so a customer can check independently of us. */
function explorerUrl(signature: string): string | null {
  if (RPC.includes("mainnet")) return `https://solscan.io/tx/${signature}`;
  if (RPC.includes("devnet")) return `https://solscan.io/tx/${signature}?cluster=devnet`;
  return null; // local node — no public explorer
}

const connection = new Connection(RPC, "confirmed");

const TYPEN: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".png": "image/png",
  ".json": "application/json; charset=utf-8",
  ".ico": "image/x-icon",
};

function json(res: http.ServerResponse, code: number, body: unknown): void {
  const roh = JSON.stringify(body);
  res.writeHead(code, {
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store",
  });
  res.end(roh);
}

async function pruefen(hash: string) {
  const sauber = hash.trim().toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(sauber)) {
    return { status: "ungueltig", grund: "Kein SHA-256-Fingerabdruck." };
  }

  const eintrag = proofStore.find(sauber);
  if (!eintrag) {
    // Nicht unterscheidbar von "nachträglich verändert" - und genau so wird
    // es auf der Seite auch gesagt. Alles andere wäre eine Behauptung.
    return { status: "unbekannt" };
  }

  // Nie dem eigenen Register vertrauen: gegen die Kette gegenprüfen.
  try {
    const aufDerKette = await readProof(connection, eintrag.signature);
    if (aufDerKette.hashHex !== sauber) {
      return { status: "abweichung", signature: eintrag.signature };
    }
    return {
      status: "registriert",
      signature: eintrag.signature,
      blockTime: aufDerKette.blockTime ?? eintrag.blockTime,
      slot: aufDerKette.slot,
      explorer: explorerUrl(eintrag.signature),
    };
  } catch {
    // Kette nicht erreichbar: lieber ehrlich sagen als raten.
    return { status: "kette_offline", signature: eintrag.signature };
  }
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", `http://${req.headers.host}`);

  if (url.pathname === "/api/pruefen") {
    const hash = url.searchParams.get("hash") ?? "";
    json(res, 200, await pruefen(hash));
    return;
  }

  if (url.pathname === "/api/status") {
    json(res, 200, {
      nachweise: proofStore.count(),
      kette: RPC,
      netz: RPC.includes("mainnet") ? "mainnet" : RPC.includes("devnet") ? "devnet" : "lokal",
    });
    return;
  }

  // Statische Dateien - bewusst ohne Pfadzusammensetzung aus der Anfrage.
  const name = url.pathname === "/" ? "pruefen.html" : path.basename(url.pathname);
  const datei = path.join(PUBLIC_DIR, name);
  if (fs.existsSync(datei) && fs.statSync(datei).isFile()) {
    res.writeHead(200, { "Content-Type": TYPEN[path.extname(name)] ?? "application/octet-stream" });
    fs.createReadStream(datei).pipe(res);
    return;
  }

  res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
  res.end("Nicht gefunden");
});

if (require.main === module) {
  server.listen(PORT, () => {
    console.log(`Prüfseite läuft auf http://127.0.0.1:${PORT}`);
    console.log(`Kette: ${RPC}`);
    console.log(`Register: ${proofStore.count()} Nachweise`);
  });
}

export { server, pruefen };
