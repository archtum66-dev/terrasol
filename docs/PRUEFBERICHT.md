# Prüfbericht — Lean MVP, vollständig durchgelaufen

23./24. August 2026 · Alles unten wurde ausgeführt, nicht behauptet.

Kein Knopfdruck nötig. Die Prüfung lief gegen eine **echte Solana-Laufzeit** –
einen lokalen Knoten mit denselben Programmen wie Mainnet (System, SPL Token,
Memo) und dem von Mainnet geklonten Metaplex-Programm.

```
solana-test-validator --reset --quiet \
  --clone-upgradeable-program metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s \
  --url https://api.mainnet-beta.solana.com
```

Damit fällt die Abhängigkeit vom öffentlichen Devnet-Faucet weg. Der ist
regelmässig ausgeschöpft und antwortet mit «Internal error» oder HTTP 429 –
geprüft an fünf Endpunkten, alle verweigerten.

---

## 1 · Verifizierungs-Pipeline — 7 von 7

`npm run test:verify` (dein bestehender Code, unverändert)

| Prüfung | |
|---|---|
| gültiger stillgelegter Credit angenommen | PASS |
| Menge korrekt in Gramm umgerechnet | PASS |
| Nachweis-Hash deterministisch | PASS |
| Überzeichnung abgewiesen | PASS |
| nicht stillgelegter Credit abgewiesen | PASS |
| unbekannte Seriennummer abgewiesen | PASS |
| Doppelzählung nach Commit abgewiesen | PASS |

## 2 · TRRA geprägt — vier Transaktionen

`python token_erstellen.py --lokal`

| | |
|---|---|
| Mint | `UzWvBJKArjnz2MFbA4ScfCJNr2p3Y7t7eUddyArjxF1` |
| Metadatenkonto | `DYosXtXauKsSpHnttrcui6cfiTTVCKQJkeJAAzh1rmgM` |
| Tokenkonto | `4j49vG9Fpv9GLZaJzaTcdSBatb3xm72MNwakWeak65VZ` |

**Von der Kette zurückgelesen** — nicht aus unserer eigenen Datei:

```
Dezimalen        9
Menge            100'000'000
Mint-Recht       abgegeben (feste Menge)
Freeze-Recht     abgegeben
Name / Symbol    TerraSol / TRRA
```

Damit ist auch belegt, dass die von Hand gebaute Metaplex-Anweisung vom echten
Metaplex-Programm angenommen wird.

## 3 · On-Chain-Anker — 13 von 13

`npm run test:anchor`

| Prüfung | |
|---|---|
| Nutzlast-Format hält den Rundlauf aus | PASS |
| fremdes Memo wird ignoriert | PASS |
| falsche Hash-Länge abgewiesen | PASS |
| Pipeline liefert einen Nachweis | PASS |
| Hash ist 32 Byte | PASS |
| Nachweis von der Kette lesbar | PASS |
| Hash auf der Kette stimmt mit dem Dokument überein | PASS |
| Kette trägt einen Zeitstempel | PASS |
| **verändertes Dokument stimmt nicht mehr überein** | **PASS** |
| keine Personendaten auf der Kette | PASS |
| Kosten sind genau eine Transaktionsgebühr | PASS |

Beispiel-Nachweis auf der Kette:

```
Signatur:  3Joqv4915MTXTwCvbK5SbdzhXwFDupUhscdUsLAx3FZsjgQV3zSQofqgRfb6RAXnMxXoTz3iadbhihXHe2cwvLYG
Memo:      TSOL1 f6ea590bf925c9e5ddaa8ddf6073dd5791d0a7f74e38d22370e7a1bfd00fea0b
Zeit:      2026-08-23T21:04:32Z
```

**Die vorletzte Zeile ist das Produkt.** Ein Byte im Dokument geändert – der
Hash stimmt nicht mehr, die Prüfung schlägt fehl. Alles andere ist Buchhaltung.

## 4 · Kosten je Nachweis — gemessen, nicht geschätzt

| | |
|---|---:|
| bezahlt | **5000 Lamports** |
| in SOL | 0.000005 |
| in USD bei SOL 95.09 | **0.000475** |

Eine halbe Zehntelrappen pro Nachweis. Kein Programmkonto, keine Miete, kein
Deploy. Zum Vergleich: Das Anchor-Programm auf Mainnet kostet je nach Grösse
**132 bis 331 USD**, bevor ein einziger Nachweis entsteht.

## 5 · Prüfseite — 9 von 9, dazu 6 im echten Browser

`npm run test:seite` und ein Durchlauf in Chromium.

Der Kundenweg einmal ganz durch: Credit verifizieren → Nachweis auf die Kette →
ins Register → Dokument vorlegen → Antwort.

| Prüfung | |
|---|---|
| Verifizierung liefert einen Nachweis | PASS |
| Nachweis im Register | PASS |
| richtiges Dokument → registriert | PASS |
| Zeitstempel kommt von der Kette | PASS |
| Signatur wird mitgeliefert | PASS |
| verändertes Dokument → nicht registriert | PASS |
| fremdes Dokument → nicht registriert | PASS |
| ungültige Eingabe abgewiesen | PASS |
| **falscher Registereintrag wird von der Kette widerlegt** | **PASS** |

Die letzte Zeile ist die wichtigste: Ein untergeschobener Eintrag in *unserer*
Datenbank hilft nichts, weil jede Antwort gegen die Kette gegengeprüft wird.
Der Dienst kann sich nicht selbst bestätigen.

Im echten Browser, mit echtem Upload:

| Prüfung | |
|---|---|
| Seite lädt | PASS |
| echtes Dokument → grün «Registriert und unverändert» | PASS |
| Zeitstempel wird gezeigt | PASS |
| Signatur wird gezeigt | PASS |
| verändertes Dokument → rot | PASS |
| keine Konsolenfehler | PASS |

Bildschirmfotos: `_bilder/pruefseite-start.png`, `-gruen.png`, `-rot.png`.

### Der Entwurfsentscheid, der zählt

**Das Dokument verlässt den Rechner des Kunden nicht.** Der Fingerabdruck wird
im Browser gerechnet (`crypto.subtle`); zum Server gehen 64 Hex-Zeichen. Das
ist nicht Höflichkeit, sondern Produkt: Ein Kunde kann ein vertrauliches
Dokument prüfen, ohne es uns zu geben — und unsere Protokolle können nichts
verlieren, was wir nie bekommen haben.

Und die Antwort bei Rot ist bewusst ehrlich: *«Entweder nie registriert — oder
nach der Registrierung verändert. Beides ist von aussen nicht unterscheidbar.»*
Alles andere wäre eine Behauptung, die ein Hash nicht hergibt.


## 6 · Smart Contract — erstmals kompiliert, deployt, 15 von 15

26. August 2026 · `bash program/alles-testen.sh` (frische Kette → Deploy → Test)

Das Anchor-Programm aus `01_Code/terrasol` war bis heute **nie kompiliert**
worden; die Program-ID war ein Platzhalter. Jetzt:

| | |
|---|---|
| Program-ID (lokal/Devnet) | `3GGT5oAJXjpvFnofn3W25jTBhKRp4TEmKSSyzm7J7E9z` |
| Grösse | 320'280 Bytes → Mainnet-Miete ≈ 2.23 SOL |
| Toolchain | Agave 4.2.1, `cargo build-sbf --arch v3` |

| Prüfung (ohne IDL — Discriminatoren von Hand) | |
|---|---|
| TRRA-Mint, Alice 5000 / Bob 1000 | PASS |
| initialize mit Tokenomics-Stufen 100/1k/10k/100k | PASS |
| Config-Discriminator und alle Felder von der Kette | PASS |
| stake 1500 → Position und Vault stimmen, Stufe 2 | PASS |
| unstake in der Sperrfrist → StillLocked 6003 | PASS |
| fremdes Oracle → UnauthorizedOracle 6009 | PASS |
| ImpactRecord: Subjekt, 800 t, Beweis-Hash | PASS |
| Marktplatz: Bob kauft für 250 TRRA, Zahlung Bob→Alice | PASS |
| Listing als verkauft markiert, Käufer Bob | PASS |
| Fremder darf nicht pausieren → 6010 | PASS |
| pausiert: stake abgewiesen → 6002; entpausen | PASS |

Dazu die Abwicklungs-Demo (`settlement_demo.py`): echte Engine gelaufen
(8.6 Mio Op/s im Container), Endzustand gehasht, auf der Kette verankert,
zurückgelesen — **0.011 Lamports je Ausführung**. Das Hyperliquid-Konzept
(Matching off-chain, Beweis on-chain), ausgeführt statt behauptet.

### Weitere Funde aus dem echten Lauf

5. **Die Platzhalter-ID `TERRAd…` musste vor dem ersten Build weichen** —
   `declare_id!` verlangt einen echten Schlüssel. Erzeugt, eingesetzt, auch
   im Original-Quelltext nachgeführt.
6. **SBPF-Versionskonflikt:** Der voll aktivierte Agave-4.2-Validator lehnt
   v0-Programme bereits ab («sbpf_version … not enabled») — Neubau mit
   `--arch v3` löste es.
7. **`konto()` las auf «finalized»** — dieselbe 32-Slot-Falle wie bei
   `getBalance`, nur an dritter Stelle: Ein soeben angelegtes Config-Konto
   war «nicht vorhanden». Auf «confirmed» gestellt.
8. **Hex-Parser im Test:** `0x177a` (=6010) wurde als `0x177` (=375)
   gelesen — der einzige Fehlercode des Programms mit einem Hex-Buchstaben.
   Das Programm war korrekt, der Test nicht.

---

## Was dabei gefunden und behoben wurde

Die ersten vier kamen im Lean-MVP-Lauf hoch, vier weitere beim Smart Contract (Abschnitt 6). Kein einziger davon wäre beim Lesen
des Codes aufgefallen.

1. **`getBalance` fragte auf «finalized»**, dem Standard des Knotens. Der
   hinkt rund 32 Slots hinterher – ein gerade eingegangener Betrag las sich als
   null, und der Lauf brach mit «zu wenig SOL» ab, obwohl das Geld da war. Auf
   einem frischen lokalen Knoten fällt das sofort auf, auf Devnet nur manchmal.
   Das ist die schlimmere Variante. Behoben: ausdrücklich «confirmed».
2. **`pruefen_auf_kette` griff auf `client`**, ein Rest aus der ersten Fassung
   mit dem Bibliotheks-Client. Der Lauf prägte den Token korrekt und stürzte
   danach beim Nachschauen ab. Behoben.
3. **Die Ablagefläche legte sich über den Text.** Ein `<label>` ist von Haus
   aus inline, Polster wirkt dort nicht auf die Zeilenhöhe. Alle Tests waren
   grün, die Seite trotzdem unbrauchbar – sichtbar **nur im Bildschirmfoto**.
   Behoben mit `display:block`. Das ist das Argument dafür, eine Oberfläche
   anzuschauen und nicht nur zu testen.
4. **Ein 404 bei jedem Aufruf** – der Browser holt sich `favicon.ico`, die es
   nicht gab. Behoben mit einem eingebetteten Symbol, jetzt ohne Zusatzabruf.

---

## Warum der Anker ohne eigenes Programm auskommt

Das Anchor-Programm in `01_Code/terrasol` liefert strukturierten Zustand auf
der Kette – Staking, Marktplatz, Governance. Es ist zugleich das Einzige, was
den Lean MVP blockiert: kompilieren, auditieren, deployen, bevor ein einziger
Nachweis entsteht.

Ein Proof-of-Impact braucht davon nichts. Die Frage, die ein Kunde, ein Prüfer
oder ein Gericht stellt, lautet: **Gab es dieses Dokument zu diesem Zeitpunkt,
unverändert?** Ein SHA-256-Hash in einer Solana-Transaktion beantwortet sie –
dauerhaft, mit Zeitstempel der Kette, nachprüfbar von jedem mit einem Block
Explorer und ganz ohne Vertrauen in uns.

Das Anchor-Programm bleibt auf der Karte. Für den ersten Franken braucht es das
nicht.

### Und der Grund, warum nur der Hash daraufkommt

Seriennummern, Projekt-IDs und Kunden-Wallets sind Personen- oder
Geschäftsdaten. Auf einer öffentlichen Kette stehen sie für immer, für alle,
und lassen sich nicht löschen – das kollidiert frontal mit revDSG und DSGVO.

Darum: **nur der Hash geht auf die Kette.** Alles andere bleibt in der
Datenbank des Dienstes. Wer das Dokument hat, rechnet den Hash nach und prüft
ihn gegen die Kette. Wer es nicht hat, erfährt nichts.

---

## Nächste Schritte

| # | Schritt | Aufwand |
|---|---|---|
| ~~1~~ | ~~`chain-memo.ts` einhängen~~ | **erledigt** |
| ~~2~~ | ~~Schlanke Prüfseite~~ | **erledigt** |
| 3 | **Erster zahlender Pilot**, Rechnung von Hand (so in `README_verify.md` vorgesehen) | **Vertrieb — jetzt dran** |
| 4 | Live-Modus der Gold-Standard-Registry gegen die echte API bestätigen | offen |
| 5 | Oracle-Schlüssel in ein KMS, nicht in eine Datei | vor Produktion |

Das Anchor-Programm kommt, wenn Staking und Marktplatz gebraucht werden – nicht
vorher.
