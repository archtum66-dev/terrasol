#!/usr/bin/env bash
# Toolchain für TerraSol: Rust, Agave (Solana), Node, Python-Pakete.
# Alles kostenlos; nichts davon berührt Mainnet oder braucht SOL.
#
# Aufruf:   bash scripts/setup-toolchain.sh
# Version:  AGAVE_VERSION=v4.2.2 (wie in der CI) - bei Bedarf überschreiben.
#
# Danach:
#   cargo test --release --manifest-path engine/Cargo.toml   # Engine
#   cargo test --manifest-path program/Cargo.toml             # Programm (nativ)
#   bash program/alles-testen.sh                              # Programm auf lokaler Kette
#   (cd oracle && npm install && npm run test:offline)        # Oracle ohne Kette
set -euo pipefail

AGAVE_VERSION="${AGAVE_VERSION:-v4.2.2}"

echo "== Rust =="
if ! command -v rustup >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
rustup component add clippy rustfmt

echo "== Agave ${AGAVE_VERSION} (solana, solana-test-validator, cargo build-sbf) =="
sh -c "$(curl -sSfL "https://release.anza.xyz/${AGAVE_VERSION}/install")"
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
solana --version
cargo build-sbf --version

echo "== Lokaler Schlüssel (nur für die lokale Kette) =="
if [ ! -f "$HOME/.config/solana/id.json" ]; then
  solana-keygen new --no-bip39-passphrase --silent
fi
solana config set --url http://127.0.0.1:8899 >/dev/null

echo "== Prüfwerkzeuge =="
cargo install cargo-audit --locked

echo "== Node (oracle/) und Python (token/, program/) =="
if command -v npm >/dev/null; then
  (cd "$(dirname "$0")/../oracle" && npm install)
else
  echo "   Node 22+ fehlt - bitte installieren (https://nodejs.org)."
fi
python3 -m pip install -r "$(dirname "$0")/../token/requirements.txt"

echo
echo "Fertig. PATH für neue Terminals:"
echo '  export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"'
