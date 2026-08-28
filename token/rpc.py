"""
Schlanker Solana-RPC-Zugang
===========================
Spricht direkt mit dem Knoten über JSON-RPC. Kein Client aus einer Bibliothek,
deren Schnittstelle sich zwischen zwei Versionen ändert - genau daran ist der
erste Anlauf gescheitert: solana-py 0.40 hat den synchronen Client entfernt.

Gebraucht werden nur sechs Aufrufe. Die schreibt man in einer halben Stunde
selbst und ist dafür unabhängig.
"""

from __future__ import annotations

import base64
import time

import requests
from solders.hash import Hash
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.transaction import Transaction

LAMPORTS = 1_000_000_000

NETZE = {
    # Eigener Knoten auf dem Rechner. Unbegrenztes Spielgeld, keine Wartezeit,
    # keine Abhaengigkeit von einem oeffentlichen Faucet - der ist regelmaessig
    # ausgeschoepft. Starten mit:
    #     solana-test-validator --clone-upgradeable-program \
    #         metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s \
    #         --url https://api.mainnet-beta.solana.com
    "lokal": "http://127.0.0.1:8899",
    "devnet": "https://api.devnet.solana.com",
    "mainnet": "https://api.mainnet-beta.solana.com",
}


class RpcFehler(RuntimeError):
    pass


class Knoten:
    def __init__(self, netz: str = "devnet", zeitlimit: float = 30.0):
        if netz not in NETZE:
            raise ValueError(f"Unbekanntes Netz: {netz}")
        self.netz = netz
        self.url = NETZE[netz]
        self.zeitlimit = zeitlimit
        self._sitzung = requests.Session()
        self._nummer = 0

    def _ruf(self, methode: str, *parameter):
        self._nummer += 1
        antwort = self._sitzung.post(
            self.url,
            json={
                "jsonrpc": "2.0",
                "id": self._nummer,
                "method": methode,
                "params": list(parameter),
            },
            timeout=self.zeitlimit,
        )
        if antwort.status_code != 200:
            raise RpcFehler(f"HTTP {antwort.status_code}: {antwort.text[:200]}")
        daten = antwort.json()
        if "error" in daten:
            raise RpcFehler(f"{methode}: {daten['error'].get('message', daten['error'])}")
        return daten["result"]

    # -- Lesen ------------------------------------------------------------
    def guthaben(self, wer: Pubkey) -> float:
        """
        Ausdrücklich mit «confirmed» fragen. Ohne Angabe antwortet der Knoten
        auf dem Stand «finalized», und der hinkt rund 32 Slots hinterher: Ein
        gerade bestätigter Zahlungseingang ist dort noch nicht sichtbar, und
        das Guthaben liest sich als 0. Auf einem frischen lokalen Knoten fällt
        das sofort auf - auf Devnet nur manchmal, was schlimmer ist.
        """
        return self._ruf("getBalance", str(wer), {"commitment": "confirmed"})["value"] / LAMPORTS

    def miete_fuer(self, bytes_anzahl: int) -> int:
        return self._ruf("getMinimumBalanceForRentExemption", bytes_anzahl)

    def blockhash(self) -> Hash:
        wert = self._ruf("getLatestBlockhash", {"commitment": "finalized"})
        return Hash.from_string(wert["value"]["blockhash"])

    def konto(self, wer: Pubkey, geparst: bool = False) -> dict | None:
        """
        Wie guthaben(): ausdrücklich «confirmed». Ohne Angabe liest der Knoten
        auf «finalized» und sieht ein soeben angelegtes Konto noch nicht -
        derselbe Fehler wie beim Guthaben, nur an zweiter Stelle. Ein Konto,
        das die eigene Transaktion gerade eben angelegt hat, wäre sonst None.
        """
        kodierung = "jsonParsed" if geparst else "base64"
        return self._ruf(
            "getAccountInfo", str(wer),
            {"encoding": kodierung, "commitment": "confirmed"},
        )["value"]

    def konto_daten(self, wer: Pubkey) -> bytes | None:
        info = self.konto(wer)
        if not info:
            return None
        return base64.b64decode(info["data"][0])

    # -- Schreiben --------------------------------------------------------
    def senden(self, anweisungen: list, zahler: Keypair, mitzeichner: list = ()) -> str:
        bh = self.blockhash()
        nachricht = Message.new_with_blockhash(anweisungen, zahler.pubkey(), bh)
        tx = Transaction([zahler, *mitzeichner], nachricht, bh)
        roh = base64.b64encode(bytes(tx)).decode()
        signatur = self._ruf(
            "sendTransaction",
            roh,
            {"encoding": "base64", "preflightCommitment": "confirmed"},
        )
        self.abwarten(signatur)
        return signatur

    def abwarten(self, signatur: str, sekunden: float = 60.0) -> None:
        ende = time.time() + sekunden
        while time.time() < ende:
            stand = self._ruf("getSignatureStatuses", [signatur], {"searchTransactionHistory": True})
            eintrag = stand["value"][0]
            if eintrag:
                if eintrag.get("err"):
                    raise RpcFehler(f"Transaktion fehlgeschlagen: {eintrag['err']}")
                if eintrag.get("confirmationStatus") in ("confirmed", "finalized"):
                    return
            time.sleep(0.6)
        raise RpcFehler(f"Zeitüberschreitung beim Bestätigen von {signatur}")

    def tanken(self, wer: Pubkey, sol: float = 1.0) -> bool:
        """
        Devnet und lokaler Knoten. Der öffentliche Devnet-Faucet ist häufig
        ausgeschöpft und antwortet mit «Internal error» oder HTTP 429 - der
        lokale Knoten gibt dagegen immer und sofort.
        """
        if self.netz == "mainnet":
            return False
        try:
            signatur = self._ruf("requestAirdrop", str(wer), int(sol * LAMPORTS))
            self.abwarten(signatur)
            return True
        except RpcFehler as fehler:
            print(f"  Airdrop fehlgeschlagen: {fehler}")
            return False
