#!/usr/bin/env bash
# 从 yan-mc/melody-launcher 拉取 UI 资源（大 PNG 不进 git / MCP push）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${ROOT}/public/assets"
BASE="https://raw.githubusercontent.com/yan-mc/melody-launcher/main/src/MelodyLauncher/Assets"

mkdir -p "$DEST/backgrounds" "$DEST/ui" "$DEST/icons"

fetch() {
  local rel="$1"
  local out="$DEST/$rel"
  if [[ -f "$out" && -s "$out" ]]; then
    echo "skip existing $rel"
    return 0
  fi
  echo "fetch $rel"
  curl -fsSL "$BASE/$rel" -o "$out"
}

fetch backgrounds/main.png
fetch backgrounds/main.jpg
fetch backgrounds/sky.png
fetch backgrounds/blocks.png
fetch ui/radio_off.png
fetch ui/radio_on.png
fetch ui/radio_off_hover.png
fetch ui/radio_on_hover.png
fetch ui/checkbox_off.png
fetch ui/checkbox_on.png
fetch ui/checkbox_off_hover.png
fetch ui/checkbox_on_hover.png
fetch ui/chevron.png
fetch icons/app.ico

# Tauri icons from ico if convert available
if command -v convert >/dev/null 2>&1; then
  mkdir -p "$ROOT/src-tauri/icons"
  convert "$DEST/icons/app.ico[0]" -resize 32x32 "$ROOT/src-tauri/icons/32x32.png" || true
  convert "$DEST/icons/app.ico[0]" -resize 128x128 "$ROOT/src-tauri/icons/128x128.png" || true
  convert "$DEST/icons/app.ico[0]" -resize 256x256 "$ROOT/src-tauri/icons/henry.w@example.net" || true
  convert "$DEST/icons/app.ico[0]" -resize 512x512 "$ROOT/src-tauri/icons/icon.png" || true
  cp -f "$DEST/icons/app.ico" "$ROOT/src-tauri/icons/icon.ico" || true
fi

echo "assets ready under $DEST"
