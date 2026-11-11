#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Packages QuakeC Studio's Windows binaries (P4-18): a portable zip and an MSI, each holding
# the desktop app, the preview host beside it (qs-engine's `locate_host` finds it there), the
# command line and the language server, with LICENSE and NOTICE. The MSI installs for every
# user under Program Files, adds a Start menu shortcut and puts the folder on the system PATH
# for `quakec-ide`; its WiX source is quakec-studio.wxs beside this script, with the version
# filled in. On a tag the release signs the executables before they are packaged; the MSI
# itself is not signed.
#
#   packaging/windows/package.sh <zip|msi> <x64|arm64> <bin-dir> <version> <out-dir>
#
# Runs in Git Bash on a Windows runner, from the repository root; the MSI needs WiX 5
# (`dotnet tool install --global wix --version 5.0.2`).
set -euo pipefail

BINARIES=(quakec-studio quakec-engine quakec-ide quakec-lsp)

usage() {
  echo "usage: $0 <zip|msi> <x64|arm64> <bin-dir> <version> <out-dir>" >&2
  exit 2
}

[ "$#" -eq 5 ] || usage
format=$1 arch=$2 bins=$3 version=$4 out=$5
case "$arch" in x64 | arm64) ;; *) usage ;; esac
mkdir -p "$out"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
for binary in "${BINARIES[@]}"; do
  cp "$bins/$binary.exe" "$stage/"
done
cp LICENSE "$stage/LICENSE.txt"
cp packaging/NOTICE.txt "$stage/NOTICE.txt"
name="quakec-studio-$version-windows-$arch"

zip() {
  local folder
  folder=$(mktemp -d)
  mkdir "$folder/$name"
  cp "$stage"/* "$folder/$name/"
  # The runner's PowerShell makes the zip; Git Bash has no zip.
  (cd "$folder" && powershell -NoProfile -Command \
    "Compress-Archive -Path '$name' -DestinationPath '$name.zip'")
  mv "$folder/$name.zip" "$out/"
  rm -rf "$folder"
}

msi() {
  # An MSI's version is three numbers, the first at most 255: 2026.11.1 is 26.11.1.
  local major=${version%%.*} rest=${version#*.}
  local msi_version="$((major % 100)).${rest%%-*}"
  sed "s/@MSI_VERSION@/$msi_version/" packaging/windows/quakec-studio.wxs \
    >"$stage/quakec-studio.wxs"
  (cd "$stage" && wix build -arch "$arch" -o "$name.msi" quakec-studio.wxs)
  mv "$stage/$name.msi" "$out/"
}

case "$format" in
zip | msi) "$format" ;;
*) usage ;;
esac
