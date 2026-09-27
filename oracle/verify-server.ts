// server/oracle/verify-server.ts
//
// The public check page plus the one endpoint behind it.
//
//   GET  /                      the page
//   GET  /api/pruefen?hash=...  is this hash anchored, and since when?
//   GET  /api/status            how many proofs, which network
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
// Hardening (this is a public endpoint):
// - No request can crash the process: URLs are parsed against a fixed base
//   (never the client's Host header) and every handler error becomes a 500.
// - Strict Content-Security-Policy: the page runs only /pruefen.js, so
//   injected markup cannot execute.
// - /api/status never reveals the RPC URL (paid RPCs carry API keys in it).
// - A per-IP rate limit protects the RPC quota behind /api/pruefen.
// - ANCHOR_SIGNERS (comma-separated public keys): if set, a proof only counts
//   when the anchoring transaction was paid for by one of these keys. Without
//   it, anyone who can edit the register could anchor the same hash
//   themselves for 5000 lamports and point the register at that transaction.
//
// Start:  npx ts-node verify-server.ts
//         env: RPC_URL, PORT, ANCHOR_SIGNERS, RATE_LIMIT_PER_MIN

import fs from "fs";
import http from "http";
import path from "path";
import { Connection } from "@solana/web3.js";

import { proofStore } from "./proof-store";
import { readProof } from "./chain-memo";

const PORT = Number(process.env.PORT ?? 8080);
const RPC = process.env.RPC_URL ?? "http://127.0.0.1:8899";
const PUBLIC_DIR = path.join(__dirname, "public");
const RATE_LIMIT_PER_MIN = Number(process.env.RATE_LIMIT_PER_MIN ?? 60);
const ANCHOR_SIGNERS = (process.env.ANCHOR_SIGNERS ?? "")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);

const NETZ = RPC.includes("mainnet") ? "mainnet" : RPC.includes("devnet") ? "devnet" : "lokal";

/** Solscan cluster suffix, so a customer can check independently of us. */
function explorerUrl(signature: string): string | null {
  if (NETZ === "mainnet") return `https://solscan.io/tx/${signature}`;
  if (NETZ === "devnet") return `https://solscan.io/tx/${signature}?cluster=devnet`;
  return null; // local node — no public explorer
}

const connection = new Connection(RPC, "confirmed");

const TYPEN: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".png": "image/png",
  ".json": "application/json; charset=utf-8",
  ".ico": "image/x-icon",
};

/** Sent with every response. */
const SICHERHEIT: Record<string, string> = {
  "Content-Security-Policy":
    "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; " +
    "img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; " +
    "frame-ancestors 'none'",
  "X-Content-Type-Options": "nosniff",
  "X-Frame-Options": "DENY",
  "Referrer-Policy": "no-referrer",
};

function senden(
  res: http.ServerResponse,
  code: number,
  kopf: Record<string, string>,
  body?: string
): void {
  res.writeHead(code, { ...SICHERHEIT, ...kopf });
  res.end(body);
}

function json(res: http.ServerResponse, code: number, body: unknown): void {
  senden(
    res,
    code,
    { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store" },
    JSON.stringify(body)
  );
}

// ---------------------------------------------------------------------------
// Rate limit: fixed one-minute window per client address, in memory.
const fenster = new Map<string, { start: number; anzahl: number }>();

function zuViele(ip: string, jetzt = Date.now()): boolean {
  if (RATE_LIMIT_PER_MIN <= 0) return false;
  if (fenster.size > 50_000) fenster.clear(); // bounded memory
  const f = fenster.get(ip);
  if (!f || jetzt - f.start >= 60_000) {
    fenster.set(ip, { start: jetzt, anzahl: 1 });
    return false;
  }
  f.anzahl += 1;
  return f.anzahl > RATE_LIMIT_PER_MIN;
}

// ---------------------------------------------------------------------------
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
  let aufDerKette;
  try {
    aufDerKette = await readProof(connection, eintrag.signature);
  } catch {
    // Kette nicht erreichbar: lieber ehrlich sagen als raten.
    return { status: "kette_offline", signature: eintrag.signature };
  }

  const hashStimmt = aufDerKette.hashHex === sauber;
  const erfolgreich = !aufDerKette.failed;
  const signiererZulaessig =
    ANCHOR_SIGNERS.length === 0 ||
    (aufDerKette.feePayer !== null && ANCHOR_SIGNERS.includes(aufDerKette.feePayer));
  if (!hashStimmt || !erfolgreich || !signiererZulaessig) {
    return { status: "abweichung", signature: eintrag.signature };
  }
  return {
    status: "registriert",
    signature: eintrag.signature,
    blockTime: aufDerKette.blockTime ?? eintrag.blockTime,
    slot: aufDerKette.slot,
    explorer: explorerUrl(eintrag.signature),
  };
}

async function behandeln(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
  if (req.method !== "GET" && req.method !== "HEAD") {
    senden(res, 405, { Allow: "GET, HEAD", "Content-Type": "text/plain; charset=utf-8" }, "Methode nicht erlaubt");
    return;
  }

  // Fixed base: the client's Host header is never parsed.
  let url: URL;
  try {
    url = new URL(req.url ?? "/", "http://pruefseite.invalid");
  } catch {
    senden(res, 400, { "Content-Type": "text/plain; charset=utf-8" }, "Ungültige Anfrage");
    return;
  }

  if (url.pathname.startsWith("/api/")) {
    const ip = req.socket.remoteAddress ?? "unbekannt";
    if (zuViele(ip)) {
      senden(res, 429, { "Retry-After": "60", "Content-Type": "text/plain; charset=utf-8" }, "Zu viele Anfragen");
      return;
    }
  }

  if (url.pathname === "/api/pruefen") {
    const hash = url.searchParams.get("hash") ?? "";
    json(res, 200, await pruefen(hash));
    return;
  }

  if (url.pathname === "/api/status") {
    json(res, 200, { nachweise: proofStore.count(), netz: NETZ });
    return;
  }

  // Statische Dateien - bewusst ohne Pfadzusammensetzung aus der Anfrage.
  const name = url.pathname === "/" ? "pruefen.html" : path.basename(url.pathname);
  const datei = path.join(PUBLIC_DIR, name);
  if (fs.existsSync(datei) && fs.statSync(datei).isFile()) {
    res.writeHead(200, {
      ...SICHERHEIT,
      "Content-Type": TYPEN[path.extname(name)] ?? "application/octet-stream",
    });
    if (req.method === "HEAD") {
      res.end();
      return;
    }
    fs.createReadStream(datei).on("error", () => res.destroy()).pipe(res);
    return;
  }

  senden(res, 404, { "Content-Type": "text/plain; charset=utf-8" }, "Nicht gefunden");
}

const server = http.createServer((req, res) => {
  behandeln(req, res).catch((e) => {
    // A failed request must never take the service down.
    console.error("Anfrage fehlgeschlagen:", e instanceof Error ? e.message : e);
    if (!res.headersSent) {
      json(res, 500, { status: "fehler" });
    } else {
      res.destroy();
    }
  });
});

if (require.main === module) {
  server.listen(PORT, () => {
    console.log(`Prüfseite läuft auf http://127.0.0.1:${PORT}`);
    console.log(`Netz: ${NETZ}`);
    console.log(`Register: ${proofStore.count()} Nachweise`);
    console.log(
      ANCHOR_SIGNERS.length
        ? `Zulässige Signierer: ${ANCHOR_SIGNERS.length}`
        : "Warnung: ANCHOR_SIGNERS nicht gesetzt - fremd verankerte Nachweise werden nicht erkannt."
    );
  });
}

export { server, pruefen, zuViele };
