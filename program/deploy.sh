#!/bin/bash
# Programm auf die lokale Kette deployen. Aufruf: bash deploy.sh
set -e
export PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
cd /home/claude/tsprog

solana config set --url http://127.0.0.1:8899 >/dev/null
if [ ! -f "$HOME/.config/solana/id.json" ]; then
  solana-keygen new --no-bip39-passphrase --silent
fi

solana airdrop 500 >/dev/null
echo "Deployer: $(solana address)  Guthaben: $(solana balance)"

solana program deploy target/deploy/terrasol.so \
  --program-id target/deploy/terrasol-keypair.json \
  --commitment confirmed

echo
solana program show "$(solana-keygen pubkey target/deploy/terrasol-keypair.json)" --commitment confirmed
