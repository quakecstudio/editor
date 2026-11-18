#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Signs an rpm of QuakeC Studio with the key of packages.quakecstudio.com (P4-40), then checks
# the signature against that key's public half, so the file is the one dnf accepts with
# `gpgcheck=1`.
#
#   packaging/linux/sign-rpm.sh <rpm>
#
# GPG_PRIVATE_KEY holds the armored private key and GPG_PASSPHRASE its passphrase
# (docs/releasing.md). The key lives in a keyring this script makes and removes. Needs gpg and
# rpm, with rpmsign.
set -euo pipefail

[ "$#" -eq 1 ] || {
  echo "usage: $0 <rpm>" >&2
  exit 2
}
rpm=$1
: "${GPG_PRIVATE_KEY:?the armored private signing key}"
: "${GPG_PASSPHRASE:?the passphrase of the signing key}"

work=$(mktemp -d)
export GNUPGHOME=$work/gnupg
trap 'gpgconf --kill gpg-agent 2>/dev/null || true; rm -rf "$work"' EXIT
install -d -m700 "$GNUPGHOME"

printf '%s\n' "$GPG_PRIVATE_KEY" | gpg --batch --quiet --import
fingerprint=$(gpg --batch --list-secret-keys --with-colons | awk -F: '/^fpr/ { print $10; exit }')
(umask 077 && printf '%s' "$GPG_PASSPHRASE" >"$work/passphrase")

# Ubuntu's rpm names /usr/bin/gpg2 as its gpg, which Ubuntu does not ship.
rpmsign --addsign \
  --define "__gpg $(command -v gpg)" \
  --define "_gpg_name $fingerprint" \
  --define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback --passphrase-file $work/passphrase" \
  "$rpm"

# The check uses a database of its own, holding only this key.
gpg --batch --armor --export "$fingerprint" >"$work/key.asc"
rpm --dbpath "$work/rpmdb" --import "$work/key.asc"
result=$(rpm --dbpath "$work/rpmdb" --checksig "$rpm")
echo "$result"
case "$result" in
*"digests signatures OK") ;;
*)
  echo "error: $rpm is not signed by $fingerprint" >&2
  exit 1
  ;;
esac
