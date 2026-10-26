#!/bin/sh
# Build kit's NK65 firmware for both PCB revisions (v1 and v1.4).
# Usage: firmware/nk65/build.sh <qmk_firmware checkout> [out dir]
# Uses the qmk CLI if it is installed (CI runs in QMK's container), else QMK's Docker image.
set -e
qmk=$(cd "$1" && pwd)
out=${2:-$(pwd)/firmware-out}
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
rm -rf "$qmk/keyboards/novelkeys/nk65/keymaps/kit"
cp -r "$here/keymaps/kit" "$qmk/keyboards/novelkeys/nk65/keymaps/kit"
build='git config --global --add safe.directory "*"; qmk compile -kb novelkeys/nk65/v1 -km kit && qmk compile -kb novelkeys/nk65/v1_4 -km kit'
if command -v qmk >/dev/null; then
  (cd "$qmk" && sh -c "$build")
else
  docker run --rm -v "$qmk":/qmk_firmware -w /qmk_firmware ghcr.io/qmk/qmk_cli sh -c "$build"
fi
cp "$qmk/novelkeys_nk65_v1_kit.bin" "$out/kit-nk65-v1.bin"
cp "$qmk/novelkeys_nk65_v1_4_kit.bin" "$out/kit-nk65-v1_4.bin"
ls -l "$out"
