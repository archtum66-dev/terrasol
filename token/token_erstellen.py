"""
Token auf Solana erstellen
==========================
Legt einen SPL-Token an: Mint einrichten, Metadaten setzen, Menge prägen und
auf Wunsch die Rechte abgeben.

    python token_erstellen.py                 Probelauf, nichts wird gesendet
    python token_erstellen.py --devnet        auf Devnet, Spielgeld
    python token_erstellen.py --mainnet       ECHT. Kostet echtes SOL.

Einstellungen stehen in konfig.json. Der Schlüssel liegt in schluessel.json und
gehört nie in ein Git und nie in eine Freigabe.

Die drei Entscheide, die zählen
-------------------------------
1. **Dezimalen.** Nachträglich nicht änderbar. 9 ist üblich (wie SOL),
   6 wie USDC. Wer hier danebengreift, muss den Token neu auflegen.
2. **Mint-Recht abgeben?** Solange es besteht, kann der Inhaber beliebig
   nachprägen. Jeder ernsthafte Käufer prüft das. Abgeben heisst: feste
   Menge, für immer.
3. **Freeze-Recht abgeben?** Solange es besteht, kann der Inhaber fremde
   Guthaben einfrieren. Für einen frei handelbaren Token ist das ein
   Ausschlusskriterium - abgeben.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.system_program import CreateAccountParams, create_account
from spl.token.constants import MINT_LEN, TOKEN_PROGRAM_ID
from spl.token.models import (
    AuthorityType,
    InitializeMintParams,
    MintToParams,
    SetAuthorityParams,
)
from spl.token.instructions import (
    create_associated_token_account,
    get_associated_token_address,
    initialize_mint,
    mint_to,
    set_authority,
)

from metaplex import metadaten_adresse, metadaten_anlegen
from rpc import LAMPORTS, NETZE, Knoten, RpcFehler

HIER = Path(__file__).resolve().parent
KONFIG = HIER / "konfig.json"
SCHLUESSEL = HIER / "schluessel.json"

# ---------------------------------------------------------------------------
def konfig_laden() -> dict:
    if not KONFIG.exists():
        sys.exit(f"konfig.json fehlt in {HIER}")
    return json.loads(KONFIG.read_text(encoding="utf-8"))


def schluessel_laden(erzeugen: bool) -> Keypair:
    """Zahler- und Autoritätsschlüssel. Wird erzeugt, wenn er fehlt."""
    if SCHLUESSEL.exists():
        roh = json.loads(SCHLUESSEL.read_text())
        return Keypair.from_bytes(bytes(roh))
    if not erzeugen:
        # Probelauf: flüchtiger Schlüssel, wird nicht gespeichert.
        return Keypair()
    kp = Keypair()
    SCHLUESSEL.write_text(json.dumps(list(bytes(kp))))
    SCHLUESSEL.chmod(0o600)
    print(f"  Neuer Schlüssel erzeugt: {kp.pubkey()}")
    print(f"  Gespeichert in {SCHLUESSEL.name} - diese Datei ist das Konto. Sichern.")
    return kp


# ---------------------------------------------------------------------------
def plan_zeigen(k: dict, netz: str, zahler: Pubkey | None) -> None:
    menge_roh = int(k["menge"]) * 10 ** int(k["dezimalen"])
    print("\n  Vorhaben")
    print("  " + "-" * 62)
    for feld, wert in [
        ("Netz", netz),
        ("Name", k["name"]),
        ("Symbol", k["symbol"]),
        ("Dezimalen", k["dezimalen"]),
        ("Menge", f'{int(k["menge"]):,}'.replace(",", "'")),
        ("Menge in kleinsten Einheiten", f"{menge_roh:,}".replace(",", "'")),
        ("Metadaten-URI", k.get("uri") or "(keine)"),
        ("Metadaten änderbar", "ja" if k.get("veraenderbar", True) else "nein"),
        ("Mint-Recht abgeben", "ja" if k.get("mint_recht_abgeben") else "NEIN"),
        ("Freeze-Recht abgeben", "ja" if k.get("freeze_recht_abgeben") else "NEIN"),
        ("Zahler", str(zahler) if zahler else "(noch keiner)"),
    ]:
        print(f"  {feld:<30} {wert}")
    print("  " + "-" * 62)

    warnungen = []
    if not k.get("mint_recht_abgeben"):
        warnungen.append(
            "Mint-Recht bleibt bestehen - du kannst jederzeit nachprägen. "
            "Käufer werten das als Risiko."
        )
    if not k.get("freeze_recht_abgeben"):
        warnungen.append(
            "Freeze-Recht bleibt bestehen - du könntest fremde Guthaben einfrieren. "
            "Für einen frei handelbaren Token ein Ausschlusskriterium."
        )
    if not k.get("uri"):
        warnungen.append("Ohne URI zeigt keine Wallet ein Bild oder eine Beschreibung.")
    for w in warnungen:
        print(f"  ! {w}")
    if warnungen:
        print()


# ---------------------------------------------------------------------------
def erstellen(k: dict, netz: str, echt: bool) -> dict | None:
    knoten = Knoten(netz)
    zahler = schluessel_laden(erzeugen=echt)
    plan_zeigen(k, netz, zahler.pubkey())

    if not echt:
        print("  PROBELAUF - es wird nichts gesendet.")
        print("  Für einen echten Lauf: --devnet (Spielgeld) oder --mainnet (echt).")
        return None

    stand = knoten.guthaben(zahler.pubkey())
    print(f"\n  Guthaben: {stand:.4f} SOL")
    if stand < 0.05:
        if netz in ("devnet", "lokal"):
            print("  Zu wenig - fordere Spielgeld an ...")
            knoten.tanken(zahler.pubkey(), 1.0)
            time.sleep(2)
            stand = knoten.guthaben(zahler.pubkey())
            print(f"  Guthaben jetzt: {stand:.4f} SOL")
        if stand < 0.05:
            sys.exit(
                f"  Zu wenig SOL. Sende mindestens 0.05 SOL an {zahler.pubkey()} "
                "und starte erneut."
            )

    dezimalen = int(k["dezimalen"])
    menge_roh = int(k["menge"]) * 10**dezimalen
    mint = Keypair()
    miete = knoten.miete_fuer(MINT_LEN)

    # --- Schritt 1: Mint anlegen und einrichten --------------------------
    print(f"\n  1) Mint anlegen: {mint.pubkey()}")
    schritt1 = [
        create_account(
            CreateAccountParams(
                from_pubkey=zahler.pubkey(),
                to_pubkey=mint.pubkey(),
                lamports=miete,
                space=MINT_LEN,
                owner=TOKEN_PROGRAM_ID,
            )
        ),
        initialize_mint(
            InitializeMintParams(
                program_id=TOKEN_PROGRAM_ID,
                mint=mint.pubkey(),
                decimals=dezimalen,
                mint_authority=zahler.pubkey(),
                freeze_authority=zahler.pubkey(),
            )
        ),
    ]
    sig1 = knoten.senden(schritt1, zahler, [mint])
    print(f"     {sig1}")

    # --- Schritt 2: Metadaten -------------------------------------------
    print("  2) Metadaten setzen (Name und Symbol sichtbar machen)")
    schritt2 = [
        metadaten_anlegen(
            mint=mint.pubkey(),
            mint_autoritaet=zahler.pubkey(),
            zahler=zahler.pubkey(),
            aktualisier_autoritaet=zahler.pubkey(),
            name=k["name"],
            symbol=k["symbol"],
            uri=k.get("uri", ""),
            veraenderbar=bool(k.get("veraenderbar", True)),
        )
    ]
    sig2 = knoten.senden(schritt2, zahler)
    print(f"     {sig2}")

    # --- Schritt 3: Konto anlegen und Menge prägen -----------------------
    konto = get_associated_token_address(zahler.pubkey(), mint.pubkey())
    print(f"  3) Tokenkonto {konto} anlegen und {int(k['menge']):,} Stück prägen"
          .replace(",", "'"))
    schritt3 = [
        create_associated_token_account(zahler.pubkey(), zahler.pubkey(), mint.pubkey()),
        mint_to(
            MintToParams(
                program_id=TOKEN_PROGRAM_ID,
                mint=mint.pubkey(),
                dest=konto,
                mint_authority=zahler.pubkey(),
                amount=menge_roh,
            )
        ),
    ]
    sig3 = knoten.senden(schritt3, zahler)
    print(f"     {sig3}")

    # --- Schritt 4: Rechte abgeben --------------------------------------
    sig4 = None
    rechte = []
    if k.get("mint_recht_abgeben"):
        rechte.append(
            set_authority(
                SetAuthorityParams(
                    program_id=TOKEN_PROGRAM_ID,
                    account=mint.pubkey(),
                    authority=AuthorityType.MINT_TOKENS,
                    current_authority=zahler.pubkey(),
                    new_authority=None,
                )
            )
        )
    if k.get("freeze_recht_abgeben"):
        rechte.append(
            set_authority(
                SetAuthorityParams(
                    program_id=TOKEN_PROGRAM_ID,
                    account=mint.pubkey(),
                    authority=AuthorityType.FREEZE_ACCOUNT,
                    current_authority=zahler.pubkey(),
                    new_authority=None,
                )
            )
        )
    if rechte:
        print(f"  4) {len(rechte)} Recht(e) unwiderruflich abgeben")
        sig4 = knoten.senden(rechte, zahler)
        print(f"     {sig4}")
    else:
        print("  4) Keine Rechte abgegeben (siehe Warnung oben)")

    ergebnis = {
        "netz": netz,
        "mint": str(mint.pubkey()),
        "metadaten": str(metadaten_adresse(mint.pubkey())),
        "tokenkonto": str(konto),
        "inhaber": str(zahler.pubkey()),
        "name": k["name"],
        "symbol": k["symbol"],
        "dezimalen": dezimalen,
        "menge": int(k["menge"]),
        "mint_recht_abgegeben": bool(k.get("mint_recht_abgeben")),
        "freeze_recht_abgegeben": bool(k.get("freeze_recht_abgeben")),
        "signaturen": [s for s in (sig1, sig2, sig3, sig4) if s],
    }
    (HIER / f"token_{netz}.json").write_text(
        json.dumps(ergebnis, indent=2), encoding="utf-8"
    )
    return ergebnis


def pruefen_auf_kette(netz: str, mint_adresse: str) -> None:
    """Nachschauen, was wirklich auf der Kette steht - nicht was wir glauben."""
    knoten = Knoten(netz)
    mint = Pubkey.from_string(mint_adresse)
    info = knoten.konto(mint, geparst=True)
    if info is None:
        print("  Mint nicht gefunden.")
        return
    d = info["data"]["parsed"]["info"]
    print("\n  Auf der Kette steht:")
    print(f"    Dezimalen        {d['decimals']}")
    print(f"    Menge            {int(d['supply']) / 10 ** d['decimals']:,.0f}"
          .replace(",", "'"))
    print(f"    Mint-Recht       {d.get('mintAuthority') or 'abgegeben (feste Menge)'}")
    print(f"    Freeze-Recht     {d.get('freezeAuthority') or 'abgegeben'}")
    roh = knoten.konto_daten(metadaten_adresse(mint))
    if roh:
        # Name und Symbol stehen nach 1 Byte Kennung + 2 Pubkeys.
        p = 1 + 32 + 32
        laenge = int.from_bytes(roh[p:p + 4], "little")
        name = roh[p + 4:p + 4 + laenge].decode("utf-8", "ignore").rstrip("\x00")
        p += 4 + laenge
        laenge = int.from_bytes(roh[p:p + 4], "little")
        symbol = roh[p + 4:p + 4 + laenge].decode("utf-8", "ignore").rstrip("\x00")
        print(f"    Name / Symbol    {name} / {symbol}")
    else:
        print("    Metadaten        keine")


# ---------------------------------------------------------------------------
def main() -> None:
    p = argparse.ArgumentParser(description="SPL-Token auf Solana erstellen")
    gruppe = p.add_mutually_exclusive_group()
    gruppe.add_argument("--lokal", action="store_true",
                        help="eigener Knoten auf 127.0.0.1:8899, unbegrenztes Spielgeld")
    gruppe.add_argument("--devnet", action="store_true", help="Devnet, Spielgeld")
    gruppe.add_argument("--mainnet", action="store_true", help="ECHT, kostet SOL")
    p.add_argument("--pruefen", metavar="MINT", help="Bestehenden Mint nachschauen")
    args = p.parse_args()

    netz = "mainnet" if args.mainnet else ("lokal" if args.lokal else "devnet")

    if args.pruefen:
        pruefen_auf_kette(netz, args.pruefen)
        return

    if args.mainnet:
        print("\n  ACHTUNG: Mainnet. Das kostet echtes SOL und ist nicht rückgängig.")
        if input("  Wirklich? Tippe JA: ").strip() != "JA":
            sys.exit("  Abgebrochen.")

    ergebnis = erstellen(konfig_laden(), netz,
                         echt=args.devnet or args.mainnet or args.lokal)
    if ergebnis:
        print("\n  FERTIG")
        print(f"    Mint       {ergebnis['mint']}")
        print(f"    Tokenkonto {ergebnis['tokenkonto']}")
        print(f"    Gespeichert in token_{netz}.json")
        pruefen_auf_kette(netz, ergebnis["mint"])
        if netz == "devnet":
            print(f"\n    Ansehen: https://solscan.io/token/{ergebnis['mint']}?cluster=devnet")
        else:
            print(f"\n    Ansehen: https://solscan.io/token/{ergebnis['mint']}")


if __name__ == "__main__":
    main()
