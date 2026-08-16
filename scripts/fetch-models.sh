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
# The model set is COUPLED to the core tag the calling app pins: core looks these
# files up by exact name (`scan::model_files`), so fetching the wrong set fails at
# session load with a missing file. v2 replaced the textline-orientation model
# with PP-LCNet_x0_25 in core v0.10.0. An app still pinned to core <= v0.9.4 wants
# `ocr-models-v1` and its PP-LCNet_x1_0_textline_ori.onnx — move this pointer and
# that app's core tag in the same commit, never one without the other.
base="https://github.com/Endle/beanbeaver-core/releases/download/ocr-models-v2"
for m in PP-OCRv5_mobile_det.onnx PP-OCRv5_mobile_rec.onnx PP-LCNet_x0_25_textline_ori.onnx; do
  out="$MODELS_DIR/$m"
  if [ -f "$out" ]; then
    echo "have $out"
    continue
  fi
  echo "fetch $m → $out"
  curl -sSfL -o "$out" "$base/$m"
done
echo "models in $MODELS_DIR"
