#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Packages QuakeC Studio's Linux binaries (P4-18): a deb, an rpm, an AppImage and a Flatpak,
# each holding the desktop app, the preview host beside it (qs-engine's `locate_host` finds it
# there), the command line and the language server, with LICENSE and NOTICE.
#
#   packaging/linux.sh <deb|rpm|appimage|flatpak> <x86_64|aarch64> <bin-dir> <version> <out-dir>
#
# <bin-dir> holds the four release binaries; <version> is the workspace's, such as 2026.11.0.
# Run from the repository root. The deb needs dpkg-deb, the rpm rpmbuild, the AppImage curl,
# and the Flatpak flatpak-builder with the Freedesktop 24.08 runtime and SDK installed.
set -euo pipefail

APP_ID=com.quakecstudio.QuakeCStudio
BINARIES=(quakec-studio preview-host quakec-ide quakec-lsp)
# The commands on the PATH; the preview host is started by the app, not by hand.
COMMANDS=(quakec-studio quakec-ide quakec-lsp)
SUMMARY="QuakeC IDE with its own compiler, live in-game preview and a built-in agent"
HOMEPAGE=https://github.com/quakecstudio/editor

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
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

# The .desktop entry, shared by every format.
desktop_entry() {
  cat <<EOF
[Desktop Entry]
Type=Application
Name=QuakeC Studio
Comment=$SUMMARY
Exec=quakec-studio %f
Icon=$APP_ID
Terminal=false
Categories=Development;IDE;
StartupWMClass=quakec-studio
EOF
}

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
  desktop_entry >"$prefix/share/applications/$APP_ID.desktop"
  install -m644 "$root/packaging/$APP_ID.svg" "$prefix/share/icons/hicolor/scalable/apps/"
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
  cat >"$tree/DEBIAN/control" <<EOF
Package: quakec-studio
Version: $version
Architecture: $debarch
Maintainer: QuakeC Studio contributors <https://github.com/quakecstudio/editor/issues>
Depends: libc6 (>= 2.35), libasound2t64 | libasound2, libxkbcommon0
Recommends: libvulkan1, mesa-vulkan-drivers, libgl1, libwayland-client0, libxcursor1, libxrandr2, libxi6, gnome-keyring
Section: devel
Priority: optional
Homepage: $HOMEPAGE
Description: $SUMMARY
 QuakeC Studio is an IDE for QuakeC with its own compiler, a live in-game preview built on
 its own QuakeC VM, a built-in agent and a map editor. Quake's game data is not included.
EOF
  dpkg-deb --root-owner-group --build "$tree" "$out/quakec-studio-$version-linux-$arch.deb"
}

rpm() {
  local top=$work/rpm
  mkdir -p "$top"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
  install_tree "$work/rpm-tree/usr"
  cat >"$top/SPECS/quakec-studio.spec" <<EOF
Name: quakec-studio
Version: $version
Release: 1
Summary: $SUMMARY
License: GPL-3.0-or-later
URL: $HOMEPAGE
Requires: glibc, alsa-lib, libxkbcommon
Recommends: vulkan-loader, mesa-vulkan-drivers, mesa-libGL, gnome-keyring
AutoReqProv: no

%description
QuakeC Studio is an IDE for QuakeC with its own compiler, a live in-game preview built on its
own QuakeC VM, a built-in agent and a map editor. Quake's game data is not included.

%install
cp -a $work/rpm-tree/. %{buildroot}/

%files
%license /usr/share/doc/quakec-studio/LICENSE
%doc /usr/share/doc/quakec-studio/NOTICE
/usr/bin/quakec-studio
/usr/bin/quakec-ide
/usr/bin/quakec-lsp
/usr/lib/quakec-studio
/usr/share/applications/$APP_ID.desktop
/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg
EOF
  rpmbuild --define "_topdir $top" --define "_build_id_links none" --target "$arch" \
    -bb "$top/SPECS/quakec-studio.spec"
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
  # The AppImage starts the app from its own folder, where the preview host is beside it.
  cat >"$dir/AppRun" <<'EOF'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
case "${1:-}" in
quakec-ide | quakec-lsp)
	command=$1
	shift
	exec "$here/usr/lib/quakec-studio/$command" "$@"
	;;
esac
exec "$here/usr/lib/quakec-studio/quakec-studio" "$@"
EOF
  chmod 755 "$dir/AppRun"
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
  desktop_entry >"$build/stage/$APP_ID.desktop"
  install -m644 "$root/packaging/$APP_ID.svg" "$build/stage/"
  cp "$root/packaging/$APP_ID.yml" "$build/"
  flatpak-builder --user --force-clean --disable-rofiles-fuse --arch="$arch" \
    --repo="$build/repo" "$build/app" "$build/$APP_ID.yml"
  command flatpak build-bundle --arch="$arch" "$build/repo" \
    "$out/quakec-studio-$version-linux-$arch.flatpak" "$APP_ID"
}

case "$format" in
deb | rpm | appimage | flatpak) "$format" ;;
*) usage ;;
esac
