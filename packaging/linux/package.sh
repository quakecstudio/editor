#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Packages QuakeC Studio's Linux binaries (P4-18): a deb, an rpm, an AppImage and a Flatpak,
# each holding the desktop app, the preview host beside it (qs-engine's `locate_host` finds it
# there), the command line and the language server, with LICENSE and NOTICE.
#
#   packaging/linux/package.sh <deb|rpm|appimage|flatpak> <x86_64|aarch64> <bin-dir> <version> <out-dir>
#
# <bin-dir> holds the four release binaries; <version> is the workspace's, such as 2026.11.0.
# The files each format needs are beside this script: the desktop entry and icon, the deb's
# `deb/control`, the rpm's `quakec-studio.spec`, the AppImage's `AppRun.sh` and the Flatpak's
# manifest. Run from the repository root. The deb needs dpkg-deb, the rpm rpmbuild, the AppImage curl,
# and the Flatpak flatpak-builder with the Freedesktop 24.08 runtime and SDK installed.
set -euo pipefail

APP_ID=com.quakecstudio.QuakeCStudio
BINARIES=(quakec-studio preview-host quakec-ide quakec-lsp)
# The commands on the PATH; the preview host is started by the app, not by hand.
COMMANDS=(quakec-studio quakec-ide quakec-lsp)

# appimagetool 1.9.1 and the type2 runtime, fetched by release and checked by SHA-256.
APPIMAGETOOL_VERSION=1.9.1
declare -A APPIMAGETOOL_SHA256=(
  [x86_64]=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
  [aarch64]=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
)
# type2-runtime publishes only `continuous`: a new runtime there fails this check, and the
# pin is updated after a look at what changed.
declare -A RUNTIME_SHA256=(
  [x86_64]=617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46
  [aarch64]=9a72a744d221576c9347ec074243acc5c3c7568896b12714e0e82e9f0bf47900
)

usage() {
  echo "usage: $0 <deb|rpm|appimage|flatpak> <x86_64|aarch64> <bin-dir> <version> <out-dir>" >&2
  exit 2
}

[ "$#" -eq 5 ] || usage
format=$1 arch=$2 bins=$(realpath "$3") version=$4 out=$(realpath -m "$5")
case "$arch" in x86_64 | aarch64) ;; *) usage ;; esac
root=$(pwd)
here=$root/packaging/linux
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

# Lays the files out under the prefix $1: the binaries in lib/quakec-studio, the commands
# linked into bin, the desktop entry, the icon, LICENSE and NOTICE.
install_tree() {
  local prefix=$1
  install -d "$prefix/bin" "$prefix/lib/quakec-studio"
  for binary in "${BINARIES[@]}"; do
    install -m755 "$bins/$binary" "$prefix/lib/quakec-studio/$binary"
  done
  for command in "${COMMANDS[@]}"; do
    ln -s "../lib/quakec-studio/$command" "$prefix/bin/$command"
  done
  install -d "$prefix/share/applications" "$prefix/share/icons/hicolor/scalable/apps"
  install -m644 "$here/$APP_ID.desktop" "$prefix/share/applications/"
  install -m644 "$here/$APP_ID.svg" "$prefix/share/icons/hicolor/scalable/apps/"
  install -d "$prefix/share/doc/quakec-studio"
  install -m644 "$root/LICENSE" "$prefix/share/doc/quakec-studio/LICENSE"
  install -m644 "$root/packaging/NOTICE.txt" "$prefix/share/doc/quakec-studio/NOTICE"
}

deb() {
  local debarch
  case "$arch" in x86_64) debarch=amd64 ;; aarch64) debarch=arm64 ;; esac
  local tree=$work/deb
  install_tree "$tree/usr"
  # Debian keeps a package's license in its doc folder's `copyright`.
  cp "$tree/usr/share/doc/quakec-studio/LICENSE" "$tree/usr/share/doc/quakec-studio/copyright"
  install -d "$tree/DEBIAN"
  sed -e "s/@VERSION@/$version/" -e "s/@ARCH@/$debarch/" "$here/deb/control" \
    >"$tree/DEBIAN/control"
  dpkg-deb --root-owner-group --build "$tree" "$out/quakec-studio-$version-linux-$arch.deb"
}

rpm() {
  local top=$work/rpm
  mkdir -p "$top"/{BUILD,RPMS,SOURCES,SRPMS}
  install_tree "$work/rpm-tree/usr"
  rpmbuild --define "_topdir $top" --define "_build_id_links none" --target "$arch" \
    --define "pkg_version $version" --define "tree $work/rpm-tree" \
    -bb "$here/quakec-studio.spec"
  cp "$top/RPMS/$arch/quakec-studio-$version-1.$arch.rpm" \
    "$out/quakec-studio-$version-linux-$arch.rpm"
}

# Fetches $1 to $2 and checks its SHA-256 is $3.
fetch() {
  curl --fail --location --silent --show-error --output "$2" "$1"
  echo "$3  $2" | sha256sum --check --strict
}

appimage() {
  local dir=$work/QuakeC_Studio.AppDir
  install_tree "$dir/usr"
  install -m755 "$here/AppRun.sh" "$dir/AppRun"
  cp "$dir/usr/share/applications/$APP_ID.desktop" "$dir/"
  cp "$dir/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg" "$dir/"
  local tool=$work/appimagetool runtime=$work/runtime
  fetch "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$arch.AppImage" \
    "$tool" "${APPIMAGETOOL_SHA256[$arch]}"
  fetch "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-$arch" \
    "$runtime" "${RUNTIME_SHA256[$arch]}"
  chmod 755 "$tool"
  # The runner has no FUSE, so the tool runs from its extracted self.
  ARCH=$arch APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream --runtime-file "$runtime" \
    "$dir" "$out/quakec-studio-$version-linux-$arch.AppImage"
}

flatpak() {
  local build=$work/flatpak
  mkdir -p "$build/stage"
  for binary in "${BINARIES[@]}"; do
    install -m755 "$bins/$binary" "$build/stage/$binary"
  done
  install -m644 "$root/LICENSE" "$build/stage/LICENSE"
  install -m644 "$root/packaging/NOTICE.txt" "$build/stage/NOTICE"
  install -m644 "$here/$APP_ID.desktop" "$here/$APP_ID.svg" "$build/stage/"
  cp "$here/$APP_ID.yml" "$build/"
  flatpak-builder --user --force-clean --disable-rofiles-fuse --arch="$arch" \
    --repo="$build/repo" "$build/app" "$build/$APP_ID.yml"
  command flatpak build-bundle --arch="$arch" "$build/repo" \
    "$out/quakec-studio-$version-linux-$arch.flatpak" "$APP_ID"
}

case "$format" in
deb | rpm | appimage | flatpak) "$format" ;;
*) usage ;;
esac
