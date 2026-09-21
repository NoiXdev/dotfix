#!/usr/bin/env bash
# Generate every app and tray asset from the branding masters.
#
# Two source SVGs feed two different jobs:
#   - dotfix.svg / dotfix-drift.svg: the monochrome, black-on-transparent
#     template glyph used for the menubar tray icon. macOS inverts these
#     automatically for dark menubars and the pressed state (see
#     `icon_as_template(true)` in app/src-tauri/src/tray.rs) — they must
#     stay pure black on transparent, never pre-inverted or coloured.
#   - dotfix-icon.svg: the coloured app icon, derived from the same bar
#     geometry as dotfix.svg but with a background and inverted fill, for
#     the Dock/Finder-facing assets (32x32, 128x128, 128x128@2x, icon.icns).
#
# Requires: rsvg-convert (brew install librsvg), iconutil (macOS).
set -euo pipefail

cd "$(dirname "$0")/.."
OUT=app/src-tauri/icons
mkdir -p "$OUT"

render() { # svg size out
  rsvg-convert -w "$2" -h "$2" "$1" -o "$3"
}

# App icon sizes Tauri's bundler expects, rendered from the coloured icon.
for size in 32 128; do
  render branding/dotfix-icon.svg "$size" "$OUT/${size}x${size}.png"
done
render branding/dotfix-icon.svg 256 "$OUT/128x128@2x.png"

# Tray template images, rendered from the monochrome masters. Must remain
# black-on-transparent for icon_as_template to work.
#
# Exactly two files, and no `@2x` companions: `tray.rs` embeds these two with
# `include_bytes!` and hands the bytes to `Image::from_bytes`, which takes one
# image. Nothing reads a second resolution, so generating one only produced
# assets that looked like part of the contract and were not.
render branding/dotfix.svg 16 "$OUT/tray-quiet.png"
render branding/dotfix-drift.svg 16 "$OUT/tray-drift.png"

# .icns via an iconset, from the coloured icon.
ICONSET=$(mktemp -d)/dotfix.iconset
mkdir -p "$ICONSET"
for size in 16 32 64 128 256 512; do
  render branding/dotfix-icon.svg "$size" "$ICONSET/icon_${size}x${size}.png"
  render branding/dotfix-icon.svg "$((size * 2))" "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$OUT/icon.icns"
rm -rf "$(dirname "$ICONSET")"

echo "generated:"
ls -1 "$OUT"
