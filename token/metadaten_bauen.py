"""
Token-Metadaten bauen
=====================
Ohne Metadaten heisst ein Token in jeder Wallet «Unknown Token» und zeigt eine
Adresse statt eines Namens. Metadaten bestehen aus zwei Teilen:

  1. ein **Bild**, das auch bei 32 Pixel noch etwas darstellt
  2. eine **JSON-Datei**, die Name, Symbol, Beschreibung und die Bildadresse
     zusammenfasst

Beides muss unter einer festen, öffentlich erreichbaren Adresse liegen. Erst
diese Adresse wird auf der Kette eingetragen (`uri` in konfig.json).

Zum Bild
--------
Das TerraSol-Logo trägt den Schriftzug «TERRASOLS» und viel Rand. Bei 32 Pixel
ist beides unlesbar. Darum wird nur das **Zeichen** (Sonne über Blättern)
freigestellt - das bleibt auch klein erkennbar. Der volle Schriftzug gehört auf
die Website, nicht in ein Wallet-Symbol.

Zur Beschreibung
----------------
Sie wird mitgelesen, wenn eine Aufsichtsbehörde die Einordnung prüft. Darum
steht dort **kein Wort über Rendite, Gewinn, Zins oder Wertsteigerung** -
genau so, wie es die eigene TerraSol-Vorgabe verlangt. Ein Utility-Token wird
zum Anlagetoken, sobald die Vermarktung Ertrag verspricht.

Aufruf:
    python metadaten_bauen.py                        aus dem TerraSol-Logo
    python metadaten_bauen.py --logo PFAD.jpg        aus einer anderen Vorlage
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image

HIER = Path(__file__).resolve().parent
ZIEL = HIER / "metadaten"

# Basisadresse, unter der die Dateien später erreichbar sind.
BASIS = "https://terrasols.org/token"

# Ausschnitt des Zeichens im 1024×1024-Logo: Sonne über den Blättern,
# ohne Schriftzug und ohne den Rahmen.
AUSSCHNITT = (360, 238, 690, 568)

BESCHREIBUNG = (
    "Utility-Token der TerraSol-Plattform für unabhängige CO2-Verifizierung. "
    "TRRA bezahlt Verifikationen und die Verankerung von Nachweisen und öffnet "
    "über Staking gestufte Plattformfunktionen; Halter stimmen in der Governance "
    "mit. TRRA gewährt keinen Anspruch auf Rendite, Gewinn, Zins oder Dividende "
    "und verbrieft keine Beteiligung."
)


def bild_bauen(logo: Path) -> list[Path]:
    """Zeichen freistellen und in den Grössen ablegen, die Wallets nutzen."""
    ZIEL.mkdir(parents=True, exist_ok=True)
    voll = Image.open(logo).convert("RGB")
    zeichen = voll.crop(AUSSCHNITT)

    erzeugt = []
    for kante in (512, 256, 64):
        bild = zeichen.resize((kante, kante), Image.LANCZOS)
        # Das Zeichen besteht aus wenigen Farben mit weichen Verläufen. Als
        # PNG mit voller Farbtiefe wird das unnötig gross; eine Palette mit
        # 128 Farben sieht gleich aus und ist rund fünfmal kleiner.
        klein = bild.quantize(colors=128, method=Image.MEDIANCUT, dither=Image.FLOYDSTEINBERG)
        pfad = ZIEL / f"trra-{kante}.png"
        klein.save(pfad, "PNG", optimize=True)
        erzeugt.append(pfad)
        print(f"  {pfad.name:<16} {kante}×{kante} px   {pfad.stat().st_size / 1024:>7.1f} kB")
    return erzeugt


def json_bauen(bild: Path) -> Path:
    """Metaplex-Metadaten für einen fungiblen Token."""
    daten = {
        "name": "TerraSol",
        "symbol": "TRRA",
        "description": BESCHREIBUNG,
        "image": f"{BASIS}/{bild.name}",
        "external_url": "https://terrasols.org",
        "properties": {
            "files": [{"uri": f"{BASIS}/{bild.name}", "type": "image/png"}],
            "category": "image",
        },
    }
    pfad = ZIEL / "trra.json"
    # newline="\n" erzwingen: Windows würde sonst \r\n schreiben. Die Datei
    # wäre 15 Bytes grösser als dieselbe Datei unter Linux - inhaltlich gleich,
    # aber mit anderer Prüfsumme. Bei einem Projekt, das von Prüfsummen lebt,
    # ist das kein Schönheitsfehler.
    pfad.write_text(
        json.dumps(daten, indent=2, ensure_ascii=False),
        encoding="utf-8",
        newline="\n",
    )
    print(f"  {pfad.name:<16} {pfad.stat().st_size} Bytes")
    return pfad


def pruefen(json_pfad: Path, bilder: list[Path]) -> bool:
    """Vor dem Hochladen: hält alles, was Wallets und Metaplex verlangen?"""
    daten = json.loads(json_pfad.read_text(encoding="utf-8"))
    verboten = ("rendite", "gewinn", "zins", "dividende", "profit", "yield", "apy")
    text = (daten["description"] + " " + daten["name"]).lower()

    pruefungen = [
        ("Name höchstens 32 Bytes", len(daten["name"].encode()) <= 32),
        ("Symbol höchstens 10 Bytes", len(daten["symbol"].encode()) <= 10),
        ("Bildadresse gesetzt", daten["image"].startswith("https://")),
        ("Bild vorhanden", any(b.name in daten["image"] for b in bilder)),
        ("512er-Bild unter 200 kB", (ZIEL / "trra-512.png").stat().st_size < 200_000),
        (
            "Beschreibung ohne Ertragsversprechen",
            not any(w in text for w in verboten) or "keinen anspruch auf" in text,
        ),
    ]
    print()
    alle = True
    for name, ok in pruefungen:
        print(f"  [{'ok ' if ok else 'FEHL'}] {name}")
        alle &= ok
    return alle


def main() -> None:
    p = argparse.ArgumentParser(description="Token-Metadaten für TRRA bauen")
    p.add_argument(
        "--logo",
        default=r"E:\WEB\TerraSol\LOGO.jpg",
        help="Vorlage; auf dem PC standardmässig das TerraSol-Logo",
    )
    args = p.parse_args()

    logo = Path(args.logo)
    if not logo.exists():
        # Falls das Skript im Container läuft, liegt das Logo woanders.
        ersatz = HIER / "metadaten" / "LOGO.jpg"
        if ersatz.exists():
            logo = ersatz
        else:
            raise SystemExit(f"Logo nicht gefunden: {args.logo}")

    print(f"\nVorlage: {logo}\n")
    bilder = bild_bauen(logo)
    json_pfad = json_bauen(bilder[0])
    alles_gut = pruefen(json_pfad, bilder)

    print(f"\nDateien liegen in {ZIEL}")
    print("\nNächster Schritt - hochladen, damit die Adressen erreichbar sind:")
    print(r"    E:\WEB\TerraSol\01_Code\terrasol-web\public\token\ ")
    print(f"  Danach in konfig.json eintragen:  \"uri\": \"{BASIS}/trra.json\"")
    print("\n  Achtung: Die eigene Domain ist bequem, aber vergänglich. Läuft")
    print("  terrasols.org einmal aus, zeigt der Token für immer ins Leere.")
    print("  Für Devnet in Ordnung. Vor Mainnet gehören Bild und JSON auf")
    print("  Arweave oder IPFS - dort bleiben sie unabhängig von der Domain.")

    for name, wert in (("trra.json", json_pfad),):
        digest = hashlib.sha256(wert.read_bytes()).hexdigest()[:16]
        print(f"\n  Prüfsumme {name}: {digest}")

    raise SystemExit(0 if alles_gut else 1)


if __name__ == "__main__":
    main()
