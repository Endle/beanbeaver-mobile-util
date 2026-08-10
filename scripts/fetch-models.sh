#!/usr/bin/env bash
# Download the PP-OCRv5 mobile ONNX weights the phone apps ship.
#
# Writes into $MODELS_DIR, defaulting to `models/` under the *current working
# directory* — i.e. the consuming app's repo root, not this submodule. Run it
# from there:
#
#   ./shared/scripts/fetch-models.sh              # -> ./models/
#   MODELS_DIR=/tmp/m ./shared/scripts/fetch-models.sh
#
# Already-present files are left alone, so it is safe to re-run.
set -euo pipefail
MODELS_DIR="${MODELS_DIR:-$PWD/models}"
mkdir -p "$MODELS_DIR"
base="https://github.com/Endle/beanbeaver-core/releases/download/ocr-models-v1"
for m in PP-OCRv5_mobile_det.onnx PP-OCRv5_mobile_rec.onnx PP-LCNet_x1_0_textline_ori.onnx; do
  out="$MODELS_DIR/$m"
  if [ -f "$out" ]; then
    echo "have $out"
    continue
  fi
  echo "fetch $m → $out"
  curl -sSfL -o "$out" "$base/$m"
done
echo "models in $MODELS_DIR"
