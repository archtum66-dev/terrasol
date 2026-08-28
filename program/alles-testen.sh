#!/bin/bash
# Reproduzierbarer Gesamtlauf: frische Kette -> deployen -> Ende-zu-Ende-Test.
# Aufruf: bash alles-testen.sh
set -e
export PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
cd /home/claude/tsprog

echo "== 1/3 Frische Kette =="
pkill -9 -f test-validator 2>/dev/null || true
sleep 2
rm -rf /tmp/ledger3
setsid solana-test-validator --ledger /tmp/ledger3 --reset --quiet --rpc-port 8899 \
  </dev/null >/tmp/validator3.log 2>&1 &
for i in $(seq 1 40); do
  sleep 3
  curl -s -m 3 -X POST http://127.0.0.1:8899 -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' 2>/dev/null | grep -q '"ok"' && break
done
echo "   läuft."

echo "== 2/3 Deploy =="
bash deploy.sh | grep -E "Program Id|Data Length"

echo "== 3/3 Ende-zu-Ende-Test =="
python3 test_e2e.py
