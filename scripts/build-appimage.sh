#!/usr/bin/env bash
set -euo pipefail

APP_NAME="gTunes"
APP_ID="dev.fivves.gTunes"
BIN_NAME="gtunes"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="dev"
OUTPUT_DIR="$ROOT_DIR/dist"
APPIMAGETOOL_PATH="${APPIMAGETOOL:-}"

usage() {
  cat <<EOF
Usage: scripts/build-appimage.sh [options]

Build the current gTunes dev build into an AppImage.

Options:
  --release              Build the release profile instead of the default dev profile.
  --output-dir DIR       Write the AppImage to DIR. Defaults to ./dist.
  --appimagetool PATH    Use an existing appimagetool binary/AppImage.
  -h, --help             Show this help.

Environment:
  APPIMAGETOOL=/path/to/appimagetool  Same as --appimagetool.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      PROFILE="release"
      shift
      ;;
    --output-dir)
      OUTPUT_DIR="${2:?missing output directory}"
      shift 2
      ;;
    --appimagetool)
      APPIMAGETOOL_PATH="${2:?missing appimagetool path}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$(uname -m)" in
  x86_64|amd64)
    APPIMAGE_ARCH="x86_64"
    ;;
  aarch64|arm64)
    APPIMAGE_ARCH="aarch64"
    ;;
  *)
    echo "Unsupported AppImage architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

if [[ "$PROFILE" == "release" ]]; then
  CARGO_ARGS=(build --release)
  TARGET_DIR="$ROOT_DIR/target/release"
else
  CARGO_ARGS=(build)
  TARGET_DIR="$ROOT_DIR/target/debug"
fi

VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")"
APPDIR="$ROOT_DIR/target/appimage/$APP_NAME.AppDir"
TOOLS_DIR="$ROOT_DIR/target/appimage-tools"
APPIMAGETOOL_DOWNLOAD="$TOOLS_DIR/appimagetool-$APPIMAGE_ARCH.AppImage"

find_or_download_appimagetool() {
  if [[ -n "$APPIMAGETOOL_PATH" ]]; then
    if [[ ! -x "$APPIMAGETOOL_PATH" ]]; then
      echo "appimagetool is not executable: $APPIMAGETOOL_PATH" >&2
      exit 1
    fi
    printf '%s\n' "$APPIMAGETOOL_PATH"
    return
  fi

  if command -v appimagetool >/dev/null 2>&1; then
    command -v appimagetool
    return
  fi

  if [[ ! -x "$APPIMAGETOOL_DOWNLOAD" ]]; then
    mkdir -p "$TOOLS_DIR"
    local url="https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-$APPIMAGE_ARCH.AppImage"
    echo "Downloading appimagetool for $APPIMAGE_ARCH..." >&2
    if command -v curl >/dev/null 2>&1; then
      curl -L --fail --output "$APPIMAGETOOL_DOWNLOAD" "$url"
    elif command -v wget >/dev/null 2>&1; then
      wget -O "$APPIMAGETOOL_DOWNLOAD" "$url"
    else
      echo "Install appimagetool, curl, or wget, then rerun this script." >&2
      exit 1
    fi
    chmod +x "$APPIMAGETOOL_DOWNLOAD"
  fi

  printf '%s\n' "$APPIMAGETOOL_DOWNLOAD"
}

echo "Building $APP_NAME ($PROFILE profile)..."
(cd "$ROOT_DIR" && cargo "${CARGO_ARGS[@]}")

if [[ ! -x "$TARGET_DIR/$BIN_NAME" ]]; then
  echo "Built binary not found: $TARGET_DIR/$BIN_NAME" >&2
  exit 1
fi

echo "Assembling AppDir..."
rm -rf "$APPDIR"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/scalable/apps"

install -m 755 "$TARGET_DIR/$BIN_NAME" "$APPDIR/usr/bin/$BIN_NAME"

cat > "$APPDIR/AppRun" <<EOF
#!/usr/bin/env sh
HERE="\$(dirname "\$(readlink -f "\$0")")"
export PATH="\$HERE/usr/bin:\$PATH"
export LD_LIBRARY_PATH="\$HERE/usr/lib:\$HERE/usr/lib/$APPIMAGE_ARCH-linux-gnu:\${LD_LIBRARY_PATH:-}"
export XDG_DATA_DIRS="\$HERE/usr/share:\${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "\$HERE/usr/bin/$BIN_NAME" "\$@"
EOF
chmod +x "$APPDIR/AppRun"

cat > "$APPDIR/usr/share/applications/$APP_ID.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$APP_NAME
Comment=A GTK4/Libadwaita Jellyfin music streaming client
Exec=$BIN_NAME
Icon=$APP_ID
Terminal=false
Categories=AudioVideo;Music;Player;GTK;
StartupWMClass=$APP_ID
EOF

cat > "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg" <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">
  <rect width="128" height="128" rx="28" fill="#20262e"/>
  <circle cx="43" cy="88" r="16" fill="#79c7c5"/>
  <circle cx="86" cy="78" r="16" fill="#f2b544"/>
  <path d="M58 32v57h-8V34c0-4 3-7 7-8l42-9c5-1 9 3 9 8v51h-8V27z" fill="#f4f2ed"/>
  <path d="M58 32l42-9v16l-42 9z" fill="#e84d5b"/>
</svg>
EOF

cp "$APPDIR/usr/share/applications/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
ln -s "usr/share/icons/hicolor/scalable/apps/$APP_ID.svg" "$APPDIR/$APP_ID.svg"
ln -s "$APP_ID.svg" "$APPDIR/.DirIcon"

mkdir -p "$OUTPUT_DIR"
APPIMAGETOOL_BIN="$(find_or_download_appimagetool)"
OUTPUT="$OUTPUT_DIR/$APP_NAME-$VERSION-$PROFILE-$APPIMAGE_ARCH.AppImage"

echo "Creating AppImage..."
rm -f "$OUTPUT"
ARCH="$APPIMAGE_ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL_BIN" "$APPDIR" "$OUTPUT"
chmod +x "$OUTPUT"

echo "Wrote $OUTPUT"
echo "Note: this development AppImage expects GTK4, Libadwaita, and GStreamer runtime libraries on the target system."
