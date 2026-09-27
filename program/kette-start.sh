#!/bin/bash
# Lokale Solana-Kette starten (idempotent), mit dem TerraSol-Programm unter
# seiner deklarierten ID. Aufruf: bash program/kette-start.sh
set -uo pipefail
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd "$(dirname "$0")"

PROGRAMM_ID="3GGT5oAJXjpvFnofn3W25jTBhKRp4TEmKSSyzm7J7E9z"
LEDGER="${TMPDIR:-/tmp}/terrasol-ledger"
LOG="${TMPDIR:-/tmp}/terrasol-validator.log"

gesund() {
  curl -s -m 3 -X POST http://127.0.0.1:8899 -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' 2>/dev/null | grep -q '"ok"'
}

if gesund; then
  echo "Kette läuft bereits."
  exit 0
fi

PROGRAMM=()
if [ -f target/deploy/terrasol.so ]; then
  [ -f "$HOME/.config/solana/id.json" ] || solana-keygen new --no-bip39-passphrase --silent
  PROGRAMM=(--upgradeable-program "$PROGRAMM_ID" target/deploy/terrasol.so
            "$(solana-keygen pubkey "$HOME/.config/solana/id.json")")
else
  echo "Hinweis: target/deploy/terrasol.so fehlt - Kette startet ohne Programm (erst: cargo build-sbf --arch v3)."
fi

rm -rf "$LEDGER"
setsid solana-test-validator --ledger "$LEDGER" --reset --quiet --rpc-port 8899 "${PROGRAMM[@]}" \
  </dev/null >"$LOG" 2>&1 &

for i in $(seq 1 40); do
  sleep 3
  if gesund; then
    echo "Kette läuft nach $((i * 3))s."
    exit 0
  fi
done

echo "Kette startet nicht. Letzte Logzeilen:"
tail -5 "$LOG"
exit 1
