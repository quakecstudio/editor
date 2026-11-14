#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lays out QuakeC Studio's macOS app bundle and installer package (P4-18, #850).
#
#   packaging/macos/package.sh app <bin-dir> <version> <out-dir>   # <out-dir>/QuakeC Studio.app
#   packaging/macos/package.sh pkg <app> <version> <pkg>           # the installer package of <app>
#
# <bin-dir> holds the four arm64 release binaries. The app holds them all in
# Contents/MacOS, the preview host beside the app as qs-engine's `locate_host` looks for it,
# and LICENSE and NOTICE in Contents/Resources; its Info.plist is the one beside this script
# with the version filled in. The package installs the app in /Applications and links to its
# quakec-ide and quakec-lsp in /usr/local/bin, built with pkgbuild from components.plist and
# with productbuild from distribution.xml, both beside this script. Signing and notarization
# are the release workflow's (.github/workflows/reusable-build.yml), which signs each binary,
# then the app, then the package. Run from the repository root.
set -euo pipefail

APP_NAME="QuakeC Studio"
BINARIES=(quakec-studio quakec-engine quakec-ide quakec-lsp)
# The command-line tools linked into /usr/local/bin.
LINKED=(quakec-ide quakec-lsp)
IDENTIFIER=com.quakecstudio.QuakeCStudio

usage() {
  echo "usage: $0 app <bin-dir> <version> <out-dir> | $0 pkg <app> <version> <pkg>" >&2
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
  sed "s/@VERSION@/$version/g" packaging/macos/Info.plist >"$contents/Info.plist"
  plutil -lint "$contents/Info.plist"
}

pkg() {
  local app=$1 version=$2 pkg=$3
  local work
  work=$(mktemp -d)
  trap 'rm -rf "$work"' RETURN
  mkdir -p "$work/root/Applications" "$work/root/usr/local/bin" "$work/packages"
  # ditto keeps the app's signature and stapled ticket.
  ditto "$app" "$work/root/Applications/$APP_NAME.app"
  for tool in "${LINKED[@]}"; do
    ln -s "/Applications/$APP_NAME.app/Contents/MacOS/$tool" "$work/root/usr/local/bin/$tool"
  done
  # Extended attributes (provenance, quarantine) would ride in the payload as ._ files; the
  # signature and the stapled ticket are files in the bundle, not attributes.
  xattr -crs "$work/root"
  pkgbuild --root "$work/root" --component-plist packaging/macos/components.plist \
    --identifier "$IDENTIFIER" --version "$version" --install-location / \
    "$work/packages/quakec-studio-component.pkg"
  sed "s/@VERSION@/$version/g" packaging/macos/distribution.xml >"$work/distribution.xml"
  rm -f "$pkg"
  productbuild --distribution "$work/distribution.xml" --package-path "$work/packages" "$pkg"
}

case "${1:-}" in
app)
  [ "$#" -eq 4 ] || usage
  app "$2" "$3" "$4"
  ;;
pkg)
  [ "$#" -eq 4 ] || usage
  pkg "$2" "$3" "$4"
  ;;
*) usage ;;
esac
