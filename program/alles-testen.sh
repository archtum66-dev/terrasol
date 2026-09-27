#!/bin/bash
# Reproduzierbarer Gesamtlauf: bauen -> frische Kette mit dem Programm ->
# Ende-zu-Ende-Test. Läuft in jedem frischen Klon, ohne Programm-Keypair:
# Der Validator lädt die .so direkt unter der deklarierten Programm-ID, mit
# dem lokalen Schlüssel (~/.config/solana/id.json) als Upgrade-Autorität.
# Kostet nichts: lokale Kette, Spielgeld.
#
# Aufruf: bash program/alles-testen.sh
set -euo pipefail
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd "$(dirname "$0")"

PROGRAMM_ID="3GGT5oAJXjpvFnofn3W25jTBhKRp4TEmKSSyzm7J7E9z"
LEDGER="${TMPDIR:-/tmp}/terrasol-ledger"
LOG="${TMPDIR:-/tmp}/terrasol-validator.log"

echo "== 1/4 Bauen =="
cargo build-sbf --arch v3 2>&1 | tail -1

echo "== 2/4 Schlüssel =="
if [ ! -f "$HOME/.config/solana/id.json" ]; then
  solana-keygen new --no-bip39-passphrase --silent
fi
AUTORITAET="$(solana-keygen pubkey "$HOME/.config/solana/id.json")"
echo "   Upgrade-Autorität: $AUTORITAET"

echo "== 3/4 Frische Kette =="
pkill -f "solana-test-validator --ledger $LEDGER" 2>/dev/null || true
sleep 1
rm -rf "$LEDGER"
setsid solana-test-validator --ledger "$LEDGER" --reset --quiet --rpc-port 8899 \
  --upgradeable-program "$PROGRAMM_ID" target/deploy/terrasol.so "$AUTORITAET" \
  </dev/null >"$LOG" 2>&1 &
for i in $(seq 1 40); do
  sleep 3
  if curl -s -m 3 -X POST http://127.0.0.1:8899 -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' 2>/dev/null | grep -q '"ok"'; then
    echo "   läuft nach $((i * 3)) s."
    break
  fi
  if [ "$i" = 40 ]; then
    echo "   Kette startet nicht:"; tail -5 "$LOG"; exit 1
  fi
done
solana config set --url http://127.0.0.1:8899 >/dev/null
solana airdrop 500 >/dev/null

echo "== 4/4 Ende-zu-Ende-Test =="
python3 test_e2e.py
