#!/bin/bash
# Programm auf eine Kette deployen - mit dem Programm-Keypair der Projekt-ID.
# Für lokale Tests genügt `alles-testen.sh` (braucht kein Programm-Keypair).
#
# Aufruf: bash program/deploy.sh [RPC-URL]      (Standard: lokale Kette)
#
# Das Programm-Keypair (target/deploy/terrasol-keypair.json) gehört NIE ins
# Repository. Wer die deklarierte ID nicht besitzt, deployt unter einer
# eigenen ID - dann `declare_id!` in lib.rs und PROGRAMM in test_e2e.py anpassen.
#
# Wichtig: Direkt nach dem Deploy `initialize` mit DEMSELBEN Schlüssel
# aufrufen, der hier deployt - nur die Upgrade-Autorität darf initialisieren.
set -euo pipefail
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd "$(dirname "$0")"

URL="${1:-http://127.0.0.1:8899}"
KEYPAIR="target/deploy/terrasol-keypair.json"

if [ ! -f "$KEYPAIR" ]; then
  echo "Programm-Keypair fehlt: $KEYPAIR"; exit 1
fi
solana config set --url "$URL" >/dev/null
if [ ! -f "$HOME/.config/solana/id.json" ]; then
  solana-keygen new --no-bip39-passphrase --silent
fi
case "$URL" in
  *127.0.0.1*|*localhost*) solana airdrop 500 >/dev/null ;;
esac
echo "Deployer: $(solana address)  Guthaben: $(solana balance)"

solana program deploy target/deploy/terrasol.so --program-id "$KEYPAIR" --commitment confirmed
echo
solana program show "$(solana-keygen pubkey "$KEYPAIR")" --commitment confirmed
