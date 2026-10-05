#!/usr/bin/env bash
# HUP-S1.2 / US-1.4: rebuild the BGE embedding GGUF the app's embedding llama-server loads, from
# the BAAI weights, and check it against the pin in src-tauri/runtime-deps.sha256.
#
#   scripts/build-bge-gguf.sh <bge-dir> <out-dir>
#
# <bge-dir> holds config.json, tokenizer.json and model.safetensors of BAAI/bge-base-en-v1.5 at
# revision a5beb1e3e68b9ab74eb54cfd186867f64f240e1a (the same files the release stages at
# src-tauri/models/bge-base-en-v1.5 from bge-base-en-v1.5.tar.gz). The converter is llama.cpp tag
# b8640 (convert_hf_to_gguf.py with its own gguf-py), run as `--outtype f16`; LLAMA_CPP_DIR names a
# checkout of that tag. Needs python3 and the converter's Python packages (torch CPU, transformers,
# numpy, safetensors). Writes <out-dir>/bge-base-en-v1.5-f16.gguf and fails unless its sha256
# matches the pin.
set -euo pipefail
SRC="${1:?usage: build-bge-gguf.sh <bge-dir> <out-dir>}"
OUT="${2:?usage: build-bge-gguf.sh <bge-dir> <out-dir>}"
LLAMA="${LLAMA_CPP_DIR:?set LLAMA_CPP_DIR to a llama.cpp checkout at tag b8640}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ASSET="bge-base-en-v1.5-f16.gguf"
WEIGHTS="c7c1988aae201f80cf91a5dbbd5866409503b89dcaba877ca6dba7dd0a5167d7"
PIN="$(grep -E "^[0-9a-f]{64}  ${ASSET//./\\.}\$" "$ROOT/src-tauri/runtime-deps.sha256" | cut -d' ' -f1)"
[ -n "$PIN" ] || { echo "no pin for $ASSET in src-tauri/runtime-deps.sha256" >&2; exit 1; }
[ "$(git -C "$LLAMA" describe --tags --exact-match 2>/dev/null)" = "b8640" ] \
  || { echo "$LLAMA is not llama.cpp tag b8640" >&2; exit 1; }
echo "$WEIGHTS  $SRC/model.safetensors" | shasum -a 256 -c --strict
# The converter names the model after its input directory (general.name), so the inputs are
# copied into a directory with the model's own name: the output does not depend on where they were.
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bge-base-en-v1.5" "$OUT"
cp "$SRC/config.json" "$SRC/tokenizer.json" "$SRC/model.safetensors" "$WORK/bge-base-en-v1.5/"
HF_HUB_OFFLINE=1 python3 "$LLAMA/convert_hf_to_gguf.py" "$WORK/bge-base-en-v1.5" --outtype f16 --outfile "$OUT/$ASSET"
echo "$PIN  $OUT/$ASSET" | shasum -a 256 -c --strict
echo "built $OUT/$ASSET (matches the runtime-deps pin)"
