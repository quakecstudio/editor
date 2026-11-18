#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Adds a release's debs and rpms to a checkout of quakecstudio/packages, the APT and YUM/DNF
# repository served at https://packages.quakecstudio.com (P4-40), and writes and signs its
# metadata again over every package it holds.
#
#   packaging/linux/repository.sh <packages-dir> <repository-dir>
#
# <packages-dir> holds quakec-studio-<version>-linux-<arch>.deb and .rpm for x86_64 and
# aarch64, the rpms signed by sign-rpm.sh; <repository-dir> is the checkout. GPG_PRIVATE_KEY
# holds the armored private key and GPG_PASSPHRASE its passphrase (docs/releasing.md). Every
# signature is then checked against the public key the repository publishes,
# gpg/quakecstudio.asc, so a key that is not the published one fails here and not on a user's
# machine. Needs gpg, dpkg-scanpackages (dpkg-dev), apt-ftparchive (apt-utils) and
# createrepo_c.
set -euo pipefail

[ "$#" -eq 2 ] || {
  echo "usage: $0 <packages-dir> <repository-dir>" >&2
  exit 2
}
packages=$(realpath "$1") repository=$(realpath "$2")
: "${GPG_PRIVATE_KEY:?the armored private signing key}"
: "${GPG_PASSPHRASE:?the passphrase of the signing key}"

work=$(mktemp -d)
export GNUPGHOME=$work/gnupg
trap 'gpgconf --kill gpg-agent 2>/dev/null || true; rm -rf "$work"' EXIT
install -d -m700 "$GNUPGHOME"

printf '%s\n' "$GPG_PRIVATE_KEY" | gpg --batch --quiet --import
fingerprint=$(gpg --batch --list-secret-keys --with-colons | awk -F: '/^fpr/ { print $10; exit }')
(umask 077 && printf '%s' "$GPG_PASSPHRASE" >"$work/passphrase")
sign() {
  gpg --batch --yes --pinentry-mode loopback --passphrase-file "$work/passphrase" \
    --local-user "$fingerprint" "$@"
}

cd "$repository"
for arch in x86_64 aarch64; do
  for format in deb rpm; do
    compgen -G "$packages/quakec-studio-*-linux-$arch.$format" >/dev/null || {
      echo "error: $packages holds no $format for $arch" >&2
      exit 1
    }
  done
  # The pool names each deb as Debian does, <package>_<version>_<architecture>.deb, the
  # name dpkg-scanpackages --arch looks for.
  for deb in "$packages"/quakec-studio-*-linux-"$arch".deb; do
    # shellcheck disable=SC2016 # dpkg-deb fills in the fields, not the shell
    name=$(dpkg-deb --show --showformat '${Package}_${Version}_${Architecture}.deb' "$deb")
    cp "$deb" "apt/pool/main/$name"
  done
  cp "$packages"/quakec-studio-*-linux-"$arch".rpm "rpm/$arch/"
done

# dpkg-scanpackages runs from apt/, so each Filename is relative to
# https://packages.quakecstudio.com/apt, the root an APT source names. --multiversion lists
# every version in the pool, so `apt-get install quakec-studio=<version>` finds an older one.
for debarch in amd64 arm64; do
  index=dists/stable/main/binary-$debarch/Packages
  (cd apt && dpkg-scanpackages --multiversion --arch "$debarch" pool/main >"$index")
  gzip -9nc "apt/$index" >"apt/$index.gz"
done
# The Release before this one would otherwise be hashed into the new one.
rm -f apt/dists/stable/Release apt/dists/stable/InRelease apt/dists/stable/Release.gpg
apt-ftparchive -c apt-ftparchive.conf release apt/dists/stable >"$work/Release"
mv "$work/Release" apt/dists/stable/Release
sign --clearsign -o apt/dists/stable/InRelease apt/dists/stable/Release
sign --detach-sign --armor -o apt/dists/stable/Release.gpg apt/dists/stable/Release

for arch in x86_64 aarch64; do
  # dnf reads the XML; the SQLite copies of it would only grow the repository.
  createrepo_c --update --no-database "rpm/$arch"
  sign --detach-sign --armor -o "rpm/$arch/repodata/repomd.xml.asc" "rpm/$arch/repodata/repomd.xml"
done

# The check's keyring holds only the published key.
published=$work/published
install -d -m700 "$published"
gpg --homedir "$published" --batch --quiet --import gpg/quakecstudio.asc
verify() {
  gpg --homedir "$published" --batch --verify "$@" 2>/dev/null || {
    echo "error: $1 is not signed by the key in gpg/quakecstudio.asc" >&2
    exit 1
  }
  echo "ok    $1 is signed by the published key"
}
verify apt/dists/stable/InRelease
verify apt/dists/stable/Release.gpg apt/dists/stable/Release
for arch in x86_64 aarch64; do
  verify "rpm/$arch/repodata/repomd.xml.asc" "rpm/$arch/repodata/repomd.xml"
done
