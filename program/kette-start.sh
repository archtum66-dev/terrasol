#!/bin/bash
# Lokale Solana-Kette starten (idempotent). Aufruf: bash kette-start.sh
export PATH="/root/.local/share/solana/install/active_release/bin:$PATH"

if curl -s -m 3 -X POST http://127.0.0.1:8899 -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' 2>/dev/null | grep -q '"ok"'; then
  echo "Kette läuft bereits."
  exit 0
fi

pkill -9 -f test-validator 2>/dev/null
sleep 2
rm -rf /tmp/ledger3
setsid solana-test-validator --ledger /tmp/ledger3 --reset --quiet --rpc-port 8899 \
  </dev/null >/tmp/validator3.log 2>&1 &

for i in $(seq 1 40); do
  sleep 3
  if curl -s -m 3 -X POST http://127.0.0.1:8899 -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' 2>/dev/null | grep -q '"ok"'; then
    echo "Kette läuft nach $((i*3))s."
    exit 0
  fi
done

echo "Kette startet nicht. Letzte Logzeilen:"
tail -5 /tmp/validator3.log
exit 1
