#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Packages QuakeC Studio's Windows binaries (P4-18): a portable zip and an MSI, each holding
# the desktop app, the preview host beside it (qs-engine's `locate_host` finds it there), the
# command line and the language server, with LICENSE and NOTICE. The MSI installs for every
# user under Program Files, adds a Start menu shortcut and puts the folder on the system PATH
# for `quakec-ide`. Both are unsigned until P0-23 adds Windows signing.
#
#   packaging/windows.sh <zip|msi> <x64|arm64> <bin-dir> <version> <out-dir>
#
# Runs in Git Bash on a Windows runner, from the repository root; the MSI needs WiX 5
# (`dotnet tool install --global wix --version 5.0.2`).
set -euo pipefail

BINARIES=(quakec-studio preview-host quakec-ide quakec-lsp)
# The MSI's upgrade code: the same for every version, so a new MSI replaces the old.
UPGRADE_CODE=8C1F5E2A-6B3D-4E7F-9A0C-2D4B6F8E1A3C

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
  # An MSI's version is three numbers, the first at most 255: 2026.11.0 is 26.11.0.
  local major=${version%%.*} rest=${version#*.}
  local msi_version="$((major % 100)).${rest%%-*}"
  local components=""
  for file in "${BINARIES[@]/%/.exe}" LICENSE.txt NOTICE.txt; do
    components+="          <Component><File Source=\"$file\" /></Component>"$'\n'
  done
  cat >"$stage/quakec-studio.wxs" <<EOF
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="QuakeC Studio" Manufacturer="QuakeC Studio contributors"
      Version="$msi_version" UpgradeCode="$UPGRADE_CODE" Scope="perMachine">
    <MajorUpgrade DowngradeErrorMessage="A newer QuakeC Studio is already installed." />
    <MediaTemplate EmbedCab="yes" />
    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="QuakeC Studio">
        <ComponentGroup Id="Files">
$components          <Component Id="PathEntry" Guid="5E7A1C3B-2D4F-4A6B-8C9E-0F1A2B3C4D5E">
            <Environment Id="Path" Name="PATH" Value="[INSTALLFOLDER]" Part="last"
                Action="set" System="yes" />
          </Component>
        </ComponentGroup>
      </Directory>
    </StandardDirectory>
    <StandardDirectory Id="ProgramMenuFolder">
      <Component Id="StartMenu" Guid="3B9D7F1E-5A2C-4E8B-9D6F-1C3A5E7B9D0F">
        <Shortcut Id="AppShortcut" Name="QuakeC Studio"
            Target="[INSTALLFOLDER]quakec-studio.exe" WorkingDirectory="INSTALLFOLDER" />
        <RegistryValue Root="HKLM" Key="Software\\QuakeC Studio" Name="StartMenu"
            Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </StandardDirectory>
    <Feature Id="Main">
      <ComponentGroupRef Id="Files" />
      <ComponentRef Id="StartMenu" />
    </Feature>
  </Package>
</Wix>
EOF
  (cd "$stage" && wix build -arch "$arch" -o "$name.msi" quakec-studio.wxs)
  mv "$stage/$name.msi" "$out/"
}

case "$format" in
zip | msi) "$format" ;;
*) usage ;;
esac
