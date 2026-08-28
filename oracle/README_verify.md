# TerraSol — echtes `verify.ts` (Gold Standard) + Anti-Doppelzählung

Ersetzt den Demo-Stub durch eine echte, deterministische Prüfung gegen das
Gold-Standard-Register plus einen Ledger gegen Doppelzählung. Die exportierten
Namen (`verifyEvidence`, `evidenceHash`) bleiben gleich — `chain.ts` ändert sich
nicht, `server.ts` nur minimal.

## Dateien (nach `terrasol-mvp/server/oracle/` kopieren)

- `verify.ts` — Prüf-Pipeline (Registry-Lookup → retired? → Menge → Doppelzählung → deterministischer Hash).
- `registry.ts` — Registry-Abstraktion. Gold Standard umgesetzt (Snapshot + Live-Modus), Verra als Platzhalter.
- `ledger.ts` — Append-only-Ledger: ein Serial → höchstens ein On-Chain-Nachweis.
- `data/gold-standard-retirements.json` — Beispiel-Snapshot (Format-Vorlage).
- `test-verify.ts` — Selbsttest (6 Fälle). `npx ts-node test-verify.ts`.

## Integritäts-Modell (wichtig)

Ein gültiger Proof-of-Impact verlangt einen **retired** (stillgelegten) Credit:
Stilllegung = der Klimanutzen ist dauerhaft beansprucht und der Credit kann nicht
weiterverkauft werden. Das ist der Anker gegen Doppelverkauf. Zusätzlich sorgt der
Ledger dafür, dass derselbe Serial nur **einen** Nachweis erzeugen kann. Die Menge
wird gegen die Register-Menge geprüft (Teil-Stilllegung darf einen kleineren
Nachweis tragen, nie einen grösseren).

## Zwei Register-Modi

- `GS_MODE=snapshot` (empfohlen für den Piloten): prüft gegen eine lokale
  Export-Datei (`GS_SNAPSHOT`). Deterministisch, offline. Den Export holst du aus
  der Retirements-/Credit-Blocks-Ansicht auf registry.goldstandard.org und bringst
  ihn in das Format von `data/gold-standard-retirements.json` (CSV → JSON).
- `GS_MODE=live`: fragt einen JSON-Endpoint ab (`GS_API_BASE`, optional
  `GS_API_KEY`). **Pfad/Antwortform sind vor Gebrauch gegen die aktuelle
  GSF-Registry-API zu bestätigen** — die Felder werden in `registry.ts` in
  `normalise()` gemappt.

## Env-Variablen

```
GS_MODE=snapshot                 # oder live
GS_SNAPSHOT=./data/gold-standard-retirements.json
GS_API_BASE=https://registry.goldstandard.org   # nur live; Pfad bestätigen
GS_API_KEY=...                   # nur live, falls nötig
REQUIRE_RETIRED=true             # nur zum Testen auf false
LEDGER_PATH=./data/used-serials.json
```

## Kleine Änderungen an bestehenden Dateien

**1) `store.ts` — `Submission`-Typ um die Register-Referenz ergänzen:**

```ts
export interface Submission {
  // … bestehende Felder …
  registry?: { standard: "gold_standard" | "verra"; serial: string; projectId?: string; vintage?: number };
}
```

**2) `server.ts` — Register-Referenz beim Submit übernehmen:**

```ts
const { subject, evidenceUri, co2eTonnes, registry } = req.body ?? {};
// … in das Submission-Objekt aufnehmen:
const sub: Submission = { /* … */, registry };
```

**3) `server.ts` — nach erfolgreichem On-Chain-Write den Ledger schreiben:**

```ts
import { ledger } from "./ledger";                     // oben ergänzen

const v = await verifyEvidence(sub);                    // v.record ist bei Erfolg gesetzt
// … registerImpact(…) wie bisher …
const { txSig, index } = await registerImpact(/* … */);
ledger.commit(v.record!.standard, v.record!.serial, sub.subject, txSig);   // NEU
```

**4) Frontend `app/impact/page.tsx` — Felder für Serial/Projekt/Vintage** und im
POST-Body mitschicken:

```ts
body: JSON.stringify({
  subject: wallet.publicKey.toBase58(),
  co2eTonnes: tonnes,
  registry: { standard: "gold_standard", serial, projectId, vintage },
}),
```

## Bezahl-Gate (Pilot vs. später)

Für den ersten Piloten: manuelle Rechnung, und `/api/impact/verify` erst nach
Zahlungseingang ausführen — kein Code nötig. Später ein Gate vor `/verify`
(z. B. Stripe-Webhook setzt „bezahlt").

## Offene Punkte / ehrliche Hinweise

- Der **Live-API-Pfad** in `registry.ts` ist ein plausibler Platzhalter und muss
  gegen die aktuelle GSF-Registry-API bestätigt werden; bis dahin Snapshot nutzen.
- Der **Ledger** ist für den Piloten eine JSON-Datei; für Produktion eine DB-Tabelle
  mit UNIQUE(standard, serial).
- **Register-Zustimmung** (Tokenisierung eines Credits) ist eine separate Frage;
  für den reinen Nachweis-Dienst nicht erforderlich, vor Tokenisierung aber zu klären.
- Der **Oracle-Key** gehört in Produktion in ein KMS/HSM, nie in eine Datei.
