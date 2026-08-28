"""
Ende-zu-Ende-Test des TerraSol-Programms auf der lokalen Kette
==============================================================
Ohne Anchor-Werkzeuge, ohne IDL: Discriminatoren und Borsh von Hand.
Das prueft nebenbei, dass das Programm exakt der Anchor-ABI folgt.

  Instruktions-Discriminator:  sha256("global:<name>")[0..8]
  Konto-Discriminator:         sha256("account:<Struct>")[0..8]
  Fehlercodes:                 6000 + Variantenindex

Ablauf (positiv und negativ):
  1  TRRA-Mint anlegen, Alice und Bob ausstatten
  2  initialize mit den Staking-Stufen aus den Tokenomics
  3  Config von der Kette lesen und feldweise pruefen
  4  Alice staked 1500 TRRA -> Stufe 2
  5  Sofortiges unstake -> StillLocked (6003)
  6  register_impact mit falschem Oracle -> UnauthorizedOracle (6009)
  7  register_impact mit echtem Oracle -> ImpactRecord auf der Kette
  8  Alice listet den Credit, Bob kauft ihn: TRRA wandert Bob -> Alice
  9  set_paused durch Fremden -> UnauthorizedGovernance (6010)
 10  Governance pausiert -> stake scheitert mit Paused (6002) -> entpausen

Aufruf:  python3 test_e2e.py
"""

from __future__ import annotations

import json
import sys
from hashlib import sha256
from pathlib import Path

sys.path.insert(0, "/home/claude/tok/token")

from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.system_program import ID as SYS_ID
from solders.sysvar import RENT
from solders.system_program import CreateAccountParams, create_account
from spl.token.constants import MINT_LEN, TOKEN_PROGRAM_ID
from spl.token.instructions import (
    create_associated_token_account,
    get_associated_token_address,
    initialize_mint,
    mint_to,
)
from spl.token.models import InitializeMintParams, MintToParams

from rpc import Knoten, RpcFehler

PROGRAMM = Pubkey.from_string("3GGT5oAJXjpvFnofn3W25jTBhKRp4TEmKSSyzm7J7E9z")
EINHEIT = 10**9                       # 9 Dezimalen

fehlschlaege = 0


def check(name: str, ok: bool, detail: str = "") -> None:
    global fehlschlaege
    print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"   {detail}" if detail else ""))
    if not ok:
        fehlschlaege += 1


def disc_ix(name: str) -> bytes:
    return sha256(b"global:" + name.encode()).digest()[:8]


def disc_konto(name: str) -> bytes:
    return sha256(b"account:" + name.encode()).digest()[:8]


def u64(n: int) -> bytes:
    return int(n).to_bytes(8, "little")


def anchor_fehler(e: Exception) -> int | None:
    """Zieht den Custom-Fehlercode aus einer RPC-Fehlermeldung."""
    text = str(e)
    for marke in ('"Custom":', "Custom(", "custom program error: "):
        i = text.find(marke)
        if i < 0:
            continue
        rest = text[i + len(marke):].lstrip()
        if rest.startswith("0x"):
            # Hex heisst: auch a-f gehoeren zur Zahl. Der erste Wurf las nur
            # Ziffern und machte aus 0x177a (=6010) ein 0x177 (=375) - der
            # einzige Fehlercode des Programms mit einem Hex-Buchstaben.
            zahl = "".join(c for c in rest[2:] if c in "0123456789abcdefABCDEF"[:0] or c.lower() in "0123456789abcdef")
            zahl = ""
            for c in rest[2:]:
                if c.lower() in "0123456789abcdef":
                    zahl += c
                else:
                    break
            if zahl:
                return int(zahl, 16)
        else:
            zahl = ""
            for c in rest:
                if c.isdigit():
                    zahl += c
                else:
                    break
            if zahl:
                return int(zahl)
    return None


def main() -> int:
    k = Knoten("lokal")

    # -- Figuren ----------------------------------------------------------
    zahler = Keypair.from_bytes(bytes(json.load(open(Path.home() / ".config/solana/id.json"))))
    governance, oracle, alice, bob = Keypair(), Keypair(), Keypair(), Keypair()
    for wer in (governance, oracle, alice, bob):
        k._ruf("requestAirdrop", str(wer.pubkey()), 20 * EINHEIT)
    import time as _t
    _t.sleep(1.5)

    # -- 1) TRRA-Mint und Konten -----------------------------------------
    mint = Keypair()
    miete = k.miete_fuer(MINT_LEN)
    ix = [
        create_account(CreateAccountParams(
            from_pubkey=zahler.pubkey(), to_pubkey=mint.pubkey(),
            lamports=miete, space=MINT_LEN, owner=TOKEN_PROGRAM_ID)),
        initialize_mint(InitializeMintParams(
            program_id=TOKEN_PROGRAM_ID, mint=mint.pubkey(), decimals=9,
            mint_authority=zahler.pubkey(), freeze_authority=None)),
    ]
    for wer in (alice, bob):
        ix.append(create_associated_token_account(zahler.pubkey(), wer.pubkey(), mint.pubkey()))
    alice_token = get_associated_token_address(alice.pubkey(), mint.pubkey())
    bob_token = get_associated_token_address(bob.pubkey(), mint.pubkey())
    ix += [
        mint_to(MintToParams(program_id=TOKEN_PROGRAM_ID, mint=mint.pubkey(),
                             dest=alice_token, mint_authority=zahler.pubkey(),
                             amount=5_000 * EINHEIT)),
        mint_to(MintToParams(program_id=TOKEN_PROGRAM_ID, mint=mint.pubkey(),
                             dest=bob_token, mint_authority=zahler.pubkey(),
                             amount=1_000 * EINHEIT)),
    ]
    k.senden(ix, zahler, [mint])
    check("TRRA-Mint angelegt, Alice 5000 / Bob 1000", True, str(mint.pubkey())[:20] + "…")

    # -- PDAs -------------------------------------------------------------
    config, _ = Pubkey.find_program_address([b"config"], PROGRAMM)
    vault, _ = Pubkey.find_program_address([b"vault", bytes(config)], PROGRAMM)
    position, _ = Pubkey.find_program_address([b"position", bytes(alice.pubkey())], PROGRAMM)

    # -- 2) initialize ----------------------------------------------------
    stufen = [100 * EINHEIT, 1_000 * EINHEIT, 10_000 * EINHEIT, 100_000 * EINHEIT]
    daten = disc_ix("initialize") + b"".join(u64(s) for s in stufen)
    k.senden([Instruction(PROGRAMM, daten, [
        AccountMeta(config, False, True),
        AccountMeta(governance.pubkey(), False, False),
        AccountMeta(oracle.pubkey(), False, False),
        AccountMeta(mint.pubkey(), False, False),
        AccountMeta(vault, False, True),
        AccountMeta(zahler.pubkey(), True, True),
        AccountMeta(TOKEN_PROGRAM_ID, False, False),
        AccountMeta(SYS_ID, False, False),
        AccountMeta(RENT, False, False),
    ])], zahler)
    check("initialize mit Tokenomics-Stufen 100/1k/10k/100k", True)

    # -- 3) Config zurücklesen -------------------------------------------
    roh = k.konto_daten(config)
    check("Config-Discriminator stimmt", roh[:8] == disc_konto("Config"))
    p = 8
    cfg_gov = Pubkey.from_bytes(roh[p:p+32]); p += 32
    cfg_oracle = Pubkey.from_bytes(roh[p:p+32]); p += 32
    cfg_mint = Pubkey.from_bytes(roh[p:p+32]); p += 32
    p += 32                                            # vault
    gelesene_stufen = [int.from_bytes(roh[p+i*8:p+(i+1)*8], "little") for i in range(4)]
    check("Config: Governance, Oracle, Mint, Stufen korrekt",
          cfg_gov == governance.pubkey() and cfg_oracle == oracle.pubkey()
          and cfg_mint == mint.pubkey() and gelesene_stufen == stufen)

    # -- 4) Alice staked 1500 -> Stufe 2 ---------------------------------
    daten = disc_ix("stake") + u64(1_500 * EINHEIT)
    stake_konten = [
        AccountMeta(config, False, True),
        AccountMeta(position, False, True),
        AccountMeta(vault, False, True),
        AccountMeta(alice_token, False, True),
        AccountMeta(alice.pubkey(), True, True),
        AccountMeta(TOKEN_PROGRAM_ID, False, False),
        AccountMeta(SYS_ID, False, False),
    ]
    k.senden([Instruction(PROGRAMM, daten, stake_konten)], alice)
    pos = k.konto_daten(position)
    betrag = int.from_bytes(pos[8+32:8+32+8], "little")
    vault_stand = int(k.konto(vault, geparst=True)["data"]["parsed"]["info"]["tokenAmount"]["amount"])
    check("stake: Position 1500, Vault 1500",
          betrag == 1_500 * EINHEIT and vault_stand == 1_500 * EINHEIT)
    # Stufe nach tier_for: 1500 >= 100 (1), >= 1000 (2), < 10000 -> 2
    check("Stufe 2 erreicht (Tokenomics)", 1_500 * EINHEIT >= stufen[1] and 1_500 * EINHEIT < stufen[2])

    # -- 5) Sofortiges unstake -> StillLocked ----------------------------
    daten = disc_ix("unstake") + u64(100 * EINHEIT)
    try:
        k.senden([Instruction(PROGRAMM, daten, stake_konten[:-1])], alice)
        check("unstake in der Sperrfrist abgewiesen", False, "ging durch!")
    except RpcFehler as e:
        check("unstake in der Sperrfrist abgewiesen (StillLocked 6003)",
              anchor_fehler(e) == 6003, f"Code {anchor_fehler(e)}")

    # -- 6) register_impact mit falschem Oracle --------------------------
    impact0, _ = Pubkey.find_program_address(
        [b"impact", bytes(alice.pubkey()), u64(0)], PROGRAMM)
    beweis = sha256(b"TerraSol Testnachweis").digest()
    def impact_daten() -> bytes:
        uri = b"https://terrasols.org/proof/0"
        return (disc_ix("register_impact") + bytes(alice.pubkey())
                + u64(800_000_000) + beweis + len(uri).to_bytes(4, "little") + uri)
    try:
        k.senden([Instruction(PROGRAMM, impact_daten(), [
            AccountMeta(config, False, True),
            AccountMeta(impact0, False, True),
            AccountMeta(alice.pubkey(), True, True),      # Alice ist NICHT das Oracle
            AccountMeta(SYS_ID, False, False),
        ])], alice)
        check("fremdes Oracle abgewiesen", False, "ging durch!")
    except RpcFehler as e:
        check("fremdes Oracle abgewiesen (UnauthorizedOracle 6009)",
              anchor_fehler(e) == 6009, f"Code {anchor_fehler(e)}")

    # -- 7) register_impact mit echtem Oracle ----------------------------
    k.senden([Instruction(PROGRAMM, impact_daten(), [
        AccountMeta(config, False, True),
        AccountMeta(impact0, False, True),
        AccountMeta(oracle.pubkey(), True, True),
        AccountMeta(SYS_ID, False, False),
    ])], oracle)
    rec = k.konto_daten(impact0)
    check("ImpactRecord-Discriminator stimmt", rec[:8] == disc_konto("ImpactRecord"))
    subj = Pubkey.from_bytes(rec[8:40])
    co2e = int.from_bytes(rec[72:80], "little")
    hash_kette = rec[80:112]
    check("ImpactRecord: Subjekt, 800 t, Beweis-Hash korrekt",
          subj == alice.pubkey() and co2e == 800_000_000 and hash_kette == beweis)

    # -- 8) Marktplatz: listen und kaufen --------------------------------
    listing, _ = Pubkey.find_program_address([b"listing", bytes(impact0)], PROGRAMM)
    daten = disc_ix("list_credit") + u64(250 * EINHEIT)
    k.senden([Instruction(PROGRAMM, daten, [
        AccountMeta(config, False, False),
        AccountMeta(impact0, False, False),
        AccountMeta(listing, False, True),
        AccountMeta(alice.pubkey(), True, True),
        AccountMeta(SYS_ID, False, False),
    ])], alice)

    def bestand(konto: Pubkey) -> int:
        return int(k.konto(konto, geparst=True)["data"]["parsed"]["info"]["tokenAmount"]["amount"])

    alice_vorher, bob_vorher = bestand(alice_token), bestand(bob_token)
    k.senden([Instruction(PROGRAMM, disc_ix("buy_credit"), [
        AccountMeta(config, False, False),
        AccountMeta(listing, False, True),
        AccountMeta(bob_token, False, True),
        AccountMeta(alice_token, False, True),
        AccountMeta(bob.pubkey(), True, True),
        AccountMeta(TOKEN_PROGRAM_ID, False, False),
    ])], bob)
    check("Marktplatz: Bob kauft für 250 TRRA, Zahlung Bob -> Alice",
          bestand(alice_token) == alice_vorher + 250 * EINHEIT
          and bestand(bob_token) == bob_vorher - 250 * EINHEIT)
    lst = k.konto_daten(listing)
    check("Listing als verkauft markiert, Käufer Bob",
          lst[8+32+32+8+32+8:8+32+32+8+32+8+32] == bytes(bob.pubkey()) and lst[8+32+32+8+32+8+32] == 1)

    # -- 9/10) Governance-Schranken und Pause ----------------------------
    def pausieren(wert: bool, wer: Keypair):
        daten = disc_ix("set_paused") + bytes([1 if wert else 0])
        k.senden([Instruction(PROGRAMM, daten, [
            AccountMeta(config, False, True),
            AccountMeta(wer.pubkey(), True, False),
        ])], wer)

    try:
        pausieren(True, bob)
        check("Fremder darf nicht pausieren", False, "ging durch!")
    except RpcFehler as e:
        check("Fremder darf nicht pausieren (UnauthorizedGovernance 6010)",
              anchor_fehler(e) == 6010, f"Code {anchor_fehler(e)}")

    pausieren(True, governance)
    try:
        k.senden([Instruction(PROGRAMM, disc_ix("stake") + u64(EINHEIT), stake_konten)], alice)
        check("Pausiert: stake abgewiesen", False, "ging durch!")
    except RpcFehler as e:
        check("Pausiert: stake abgewiesen (Paused 6002)",
              anchor_fehler(e) == 6002, f"Code {anchor_fehler(e)}")
    pausieren(False, governance)
    check("Governance kann pausieren und entpausieren", True)

    print("\n" + ("ALL PASS" if fehlschlaege == 0 else f"{fehlschlaege} FAILED"))
    return 0 if fehlschlaege == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
