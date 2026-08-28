"""
Metaplex-Metadaten für einen SPL-Token
======================================
Ohne Metadaten heisst ein Token in jeder Wallet nur «Unknown Token» und zeigt
eine Adresse statt eines Namens. Name, Symbol und Bild stehen NICHT im
Mint-Konto, sondern in einem eigenen Konto des Metaplex-Programms.

Es gibt dafür keine fertige Python-Bibliothek, die man ernsthaft einsetzen
möchte - also bauen wir die Anweisung selbst. Das Datenformat ist Borsh:

    String  = u32 Länge (little endian) + UTF-8-Bytes
    Option  = 1 Byte Kennung (0 = keiner, 1 = folgt) + Inhalt
    bool    = 1 Byte

CreateMetadataAccountV3 hat die Kennziffer 33 und diesen Aufbau:

    33 | name | symbol | uri | seller_fee_bp (u16)
       | Option<Vec<Creator>> | Option<Collection> | Option<Uses>
       | is_mutable (bool) | Option<CollectionDetails>
"""

from __future__ import annotations

from solders.instruction import AccountMeta, Instruction
from solders.pubkey import Pubkey
from solders.system_program import ID as SYSTEM_PROGRAM_ID
from solders.sysvar import RENT

# Das Metaplex-Token-Metadata-Programm. Auf Mainnet und Devnet dieselbe Adresse.
METAPLEX_PROGRAM_ID = Pubkey.from_string("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s")

# Obergrenzen von Metaplex. Längere Angaben lässt die Kette nicht zu.
MAX_NAME = 32
MAX_SYMBOL = 10
MAX_URI = 200


def _string(text: str) -> bytes:
    roh = text.encode("utf-8")
    return len(roh).to_bytes(4, "little") + roh


def metadaten_adresse(mint: Pubkey) -> Pubkey:
    """Die Adresse des Metadatenkontos ergibt sich fest aus dem Mint."""
    adresse, _ = Pubkey.find_program_address(
        [b"metadata", bytes(METAPLEX_PROGRAM_ID), bytes(mint)],
        METAPLEX_PROGRAM_ID,
    )
    return adresse


def pruefen(name: str, symbol: str, uri: str) -> list[str]:
    fehler = []
    if not name:
        fehler.append("Name fehlt.")
    if len(name.encode()) > MAX_NAME:
        fehler.append(f"Name länger als {MAX_NAME} Bytes.")
    if not symbol:
        fehler.append("Symbol fehlt.")
    if len(symbol.encode()) > MAX_SYMBOL:
        fehler.append(f"Symbol länger als {MAX_SYMBOL} Bytes.")
    if len(uri.encode()) > MAX_URI:
        fehler.append(f"URI länger als {MAX_URI} Bytes.")
    return fehler


def metadaten_anlegen(
    mint: Pubkey,
    mint_autoritaet: Pubkey,
    zahler: Pubkey,
    aktualisier_autoritaet: Pubkey,
    name: str,
    symbol: str,
    uri: str = "",
    gebuehr_bp: int = 0,
    veraenderbar: bool = True,
) -> Instruction:
    """
    Baut CreateMetadataAccountV3.

    veraenderbar=False macht die Metadaten unwiderruflich fest. Das schafft
    Vertrauen, nimmt aber jede Möglichkeit, später einen Tippfehler oder ein
    kaputtes Bild zu korrigieren. Beim ersten Mal besser True lassen und erst
    nach der Kontrolle festschreiben.
    """
    fehler = pruefen(name, symbol, uri)
    if fehler:
        raise ValueError("Metadaten unzulässig: " + " ".join(fehler))

    daten = (
        bytes([33])
        + _string(name)
        + _string(symbol)
        + _string(uri)
        + gebuehr_bp.to_bytes(2, "little")
        + bytes([0])            # keine Creator-Liste
        + bytes([0])            # keine Collection
        + bytes([0])            # keine Uses
        + bytes([1 if veraenderbar else 0])
        + bytes([0])            # keine CollectionDetails
    )

    konten = [
        AccountMeta(metadaten_adresse(mint), is_signer=False, is_writable=True),
        AccountMeta(mint, is_signer=False, is_writable=False),
        AccountMeta(mint_autoritaet, is_signer=True, is_writable=False),
        AccountMeta(zahler, is_signer=True, is_writable=True),
        AccountMeta(aktualisier_autoritaet, is_signer=False, is_writable=False),
        AccountMeta(SYSTEM_PROGRAM_ID, is_signer=False, is_writable=False),
        AccountMeta(RENT, is_signer=False, is_writable=False),
    ]
    return Instruction(METAPLEX_PROGRAM_ID, daten, konten)
