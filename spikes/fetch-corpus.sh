#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fetches the spike's benchmark project: id's rerelease QuakeC at the commit pinned in
# corpus/manifest.toml (GPL-2.0-or-later, `use = "vendor"`). It is downloaded, never committed.
#
# usage: spikes/fetch-corpus.sh <dir>
set -euo pipefail

dir=${1:?usage: fetch-corpus.sh <dir>}
commit=634eefab09a77eb7b5f5ca7078ba3d8784a91142
archive_sha256=46eeb77745e9b3065562dd8691c4ee7c3d3be112e2e042454871160d42ba182c
# quakec/defs.qc, as pinned in corpus/manifest.toml.
defs_sha256=7694c006223ec43f05b923c8fe7691a6ed8298141d1562d98d6c86162de6129d

mkdir -p "$dir"
archive="$dir/quake-rerelease-qc.tar.gz"
curl -fsSL -o "$archive" "https://codeload.github.com/id-Software/quake-rerelease-qc/tar.gz/$commit"
echo "$archive_sha256  $archive" | shasum -a 256 -c -
tar xzf "$archive" -C "$dir" --strip-components 1
echo "$defs_sha256  $dir/quakec/defs.qc" | shasum -a 256 -c -
rm "$archive"
echo "rerelease QuakeC at $commit in $dir"
