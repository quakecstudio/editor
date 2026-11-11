#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lays out QuakeC Studio's macOS app bundle and disk image (P4-18).
#
#   packaging/macos.sh app <bin-dir> <version> <out-dir>   # <out-dir>/QuakeC Studio.app
#   packaging/macos.sh dmg <app> <dmg>                      # the disk image of <app>
#
# <bin-dir> holds the four universal release binaries. The app holds them all in
# Contents/MacOS, the preview host beside the app as qs-engine's `locate_host` looks for it,
# and LICENSE and NOTICE in Contents/Resources. Signing and notarization are the release
# workflow's (.github/workflows/sign.yml), which signs each binary, then the app, then the
# disk image. Run from the repository root.
set -euo pipefail

APP_ID=com.quakecstudio.QuakeCStudio
APP_NAME="QuakeC Studio"
BINARIES=(quakec-studio preview-host quakec-ide quakec-lsp)

usage() {
  echo "usage: $0 app <bin-dir> <version> <out-dir> | $0 dmg <app> <dmg>" >&2
  exit 2
}

app() {
  local bins=$1 version=$2 out=$3
  local contents="$out/$APP_NAME.app/Contents"
  rm -rf "$out/$APP_NAME.app"
  mkdir -p "$contents/MacOS" "$contents/Resources"
  for binary in "${BINARIES[@]}"; do
    install -m755 "$bins/$binary" "$contents/MacOS/$binary"
  done
  install -m644 LICENSE "$contents/Resources/LICENSE"
  install -m644 packaging/NOTICE.txt "$contents/Resources/NOTICE"
  cat >"$contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>$APP_NAME</string>
	<key>CFBundleExecutable</key>
	<string>quakec-studio</string>
	<key>CFBundleIdentifier</key>
	<string>$APP_ID</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>$APP_NAME</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$version</string>
	<key>CFBundleVersion</key>
	<string>$version</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.developer-tools</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSHumanReadableCopyright</key>
	<string>Copyright (C) 2026 QuakeC Studio contributors. GPL-3.0-or-later.</string>
</dict>
</plist>
EOF
  plutil -lint "$contents/Info.plist"
}

dmg() {
  local app=$1 dmg=$2
  local staging
  staging=$(mktemp -d)
  trap 'rm -rf "$staging"' RETURN
  cp -R "$app" "$staging/"
  ln -s /Applications "$staging/Applications"
  rm -f "$dmg"
  hdiutil create -volname "$APP_NAME" -srcfolder "$staging" -ov -format UDZO "$dmg"
}

case "${1:-}" in
app)
  [ "$#" -eq 4 ] || usage
  app "$2" "$3" "$4"
  ;;
dmg)
  [ "$#" -eq 3 ] || usage
  dmg "$2" "$3"
  ;;
*) usage ;;
esac
