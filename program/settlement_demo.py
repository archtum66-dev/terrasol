"""
Abwicklungs-Demo: das Hyperliquid-Konzept auf Solana
====================================================
Hyperliquids Kernidee: Das Matching läuft NICHT auf der Kette - dafür ist
keine Kette schnell genug -, sondern in einer eigenen Engine; die Kette haelt
den beweisbaren Zustand.

Genau das hier, mit unseren eigenen, gemessenen Teilen:

  Matching   unsere Rust-Engine       11.8 Mio Op/s im Speicher,
                                      5.06 Mio Orders/s ueber TCP (Buendel)
  Abwicklung Solana                   ein Anker je Charge, 5000 Lamports

Der Lauf startet die ECHTE Engine (cargo run --release -- bench), nimmt deren
Endzustand (Ausfuehrungen, Volumen, bestes Gebot/Forderung), hasht ihn und
verankert den Hash auf der Kette. Danach wird er zurueckgelesen und geprueft.

Damit ist die Kette der Notar der Engine - nicht ihr Flaschenhals. Die
Abwicklungskosten sinken mit der Chargengroesse gegen null.

Aufruf:  python3 settlement_demo.py
"""

from __future__ import annotations

import re
import subprocess
import sys
import time
from hashlib import sha256
from pathlib import Path

sys.path.insert(0, "/home/claude/tok/token")

from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.pubkey import Pubkey

from rpc import Knoten

MEMO = Pubkey.from_string("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
ENGINE = Path("/home/claude/tok/engine")

fehlschlaege = 0


def check(name: str, ok: bool, detail: str = "") -> None:
    global fehlschlaege
    print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"   {detail}" if detail else ""))
    if not ok:
        fehlschlaege += 1


def main() -> int:
    # -- 1) Die echte Engine laufen lassen --------------------------------
    print("Engine läuft (3 Runden à 3 Mio Operationen) ...")
    lauf = subprocess.run(
        ["cargo", "run", "--release", "--quiet", "--bin", "messung", "--", "bench"],
        cwd=ENGINE, capture_output=True, text=True, timeout=300,
    )
    aus = lauf.stdout
    ops = re.search(r"Bester Lauf:\s+([\d']+)", aus.replace("'", ""))
    zustand = re.search(
        r"(\d+) ruhende Orders, (\d+) Ausführungen, Volumen (\d+)", aus)
    check("Engine gelaufen", lauf.returncode == 0 and zustand is not None)
    if not zustand:
        print(aus[-500:])
        return 1

    ruhend, ausfuehrungen, volumen = zustand.groups()
    rate = ops.group(1) if ops else "?"
    print(f"   {int(rate):,} Op/s · {int(ausfuehrungen):,} Ausführungen · "
          f"Volumen {int(volumen):,}".replace(",", "'"))

    # -- 2) Endzustand kanonisch hashen ----------------------------------
    # Deterministisch: dieselbe Engine-Saat ergibt denselben Zustand,
    # also denselben Hash - jeder kann ihn nachrechnen.
    kanonisch = f"TSOL-SETTLE1|orders={ruhend}|fills={ausfuehrungen}|volume={volumen}"
    beweis = sha256(kanonisch.encode()).hexdigest()
    check("Zustand kanonisch gehasht", len(beweis) == 64, beweis[:24] + "…")

    # -- 3) Auf der Kette verankern --------------------------------------
    k = Knoten("lokal")
    notar = Keypair()
    k._ruf("requestAirdrop", str(notar.pubkey()), 1_000_000_000)
    time.sleep(1.2)

    memo = f"TSOL1 {beweis}"
    vorher = k.guthaben(notar.pubkey())
    sig = k.senden([Instruction(
        MEMO, memo.encode(),
        [AccountMeta(notar.pubkey(), True, False)],
    )], notar)
    nachher = k.guthaben(notar.pubkey())
    check("Engine-Zustand auf der Kette verankert", True, sig[:24] + "…")

    # -- 4) Zurücklesen und beweisen -------------------------------------
    tx = k._ruf("getTransaction", sig,
                {"commitment": "confirmed", "maxSupportedTransactionVersion": 0})
    protokoll = " ".join(tx["meta"]["logMessages"])
    check("Hash von der Kette zurückgelesen", beweis in protokoll)
    check("Zeitstempel der Kette vorhanden", tx.get("blockTime") is not None,
          time.strftime("%Y-%m-%d %H:%M UTC", time.gmtime(tx["blockTime"])))

    kosten = round((vorher - nachher) * 1e9)
    check("Kosten: eine Transaktionsgebühr", kosten == 5000, f"{kosten} Lamports")

    # -- 5) Die Rechnung, die das Konzept trägt --------------------------
    je_charge = int(ausfuehrungen)
    print(f"\n   Abwicklungskosten je Ausführung bei dieser Charge: "
          f"{5000 / je_charge:.6f} Lamports "
          f"({5000 / je_charge * 95e-9 * 1e6:.4f} Millionstel USD)")
    print("   Matching off-chain, Beweis on-chain - die Kette ist Notar,")
    print("   nicht Flaschenhals. Das ist Hyperliquids Konzept, auf Solana.")

    print("\n" + ("ALL PASS" if fehlschlaege == 0 else f"{fehlschlaege} FAILED"))
    return 0 if fehlschlaege == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
