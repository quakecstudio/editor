#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Refresh, check or verify the cached public specifications under specs/.
#
# specs/manifest.tsv is the roster. Each row names a cached file (path), the
# specification it belongs to (spec), where it comes from (url), and the sha256,
# size and fetch date of the origin bytes. Every cached file is byte-identical to
# its origin, so the sha256 of the file on disk equals the manifest's.
#
# Adding a file: append a row with path, spec and url, and "-" in the sha256,
# bytes and fetched columns, then run this script with no arguments.
#
# Usage:
#   scripts/refresh-specs.sh           fetch every row; rewrite changed files and their rows
#   scripts/refresh-specs.sh --check   report rows whose origin changed; write nothing
#   scripts/refresh-specs.sh --verify  offline: every file matches its row and has one
#
# Requires curl and shasum or sha256sum. --verify needs no network.

set -euo pipefail

readonly CURL_TIMEOUT=120
readonly HEADER=$'path\tspec\turl\tsha256\tbytes\tfetched'

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

log() { printf '%s\n' "$*" >&2; }

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

size_of() { wc -c <"$1" | tr -d ' '; }

root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
manifest="$root/specs/manifest.tsv"
[[ -f "$manifest" ]] || die "roster not found: $manifest"
[[ "$(head -n 1 "$manifest")" == "$HEADER" ]] || die "$manifest: the header must be: $HEADER"

# A row must name a file under specs/ by a relative path with no "..", so a
# manifest edit cannot make the script write outside the cache.
check_path() {
  case "$1" in
  specs/*) ;;
  *) die "$1: path must start with specs/" ;;
  esac
  case "/$1/" in
  */../* | */./*) die "$1: path must not contain . or .. segments" ;;
  esac
}

verify() {
  local path spec url sha bytes fetched failures=0 actual
  local listed="$tmp/listed"
  : >"$listed"
  while IFS=$'\t' read -r path spec url sha bytes fetched; do
    [[ "$path" == "path" ]] && continue
    check_path "$path"
    printf '%s\n' "$path" >>"$listed"
    if [[ ! -f "$root/$path" ]]; then
      log "$path: listed in the manifest but missing"
      failures=$((failures + 1))
      continue
    fi
    actual=$(sha256_of "$root/$path")
    if [[ "$actual" != "$sha" ]]; then
      log "$path: sha256 is $actual, the manifest says $sha"
      failures=$((failures + 1))
    fi
  done <"$manifest"

  while IFS= read -r path; do
    case "$path" in
    specs/README.md | specs/manifest.tsv) continue ;;
    esac
    if ! grep -qxF "$path" "$listed"; then
      log "$path: cached but not in the manifest"
      failures=$((failures + 1))
    fi
  done < <(cd "$root" && find specs -type f | LC_ALL=C sort)

  ((failures == 0)) || die "$failures cached file(s) do not match specs/manifest.tsv"
  log "verify: every cached file matches specs/manifest.tsv"
}

# Fetch every row. A row whose fetch fails or returns something other than plain
# text keeps its file and values, so one bad origin cannot damage the cache.
refresh() {
  local mode="$1" meta code ct path spec url sha bytes fetched new_sha new_bytes
  local updated=0 unchanged=0 rejected=0 today body="$tmp/body"
  today=$(date -u +%Y-%m-%d)
  printf '%s\n' "$HEADER" >"$tmp/manifest"

  while IFS=$'\t' read -r path spec url sha bytes fetched <&3; do
    [[ "$path" == "path" ]] && continue
    check_path "$path"
    if ! meta=$(curl -sSL --proto '=https' --max-time "$CURL_TIMEOUT" --retry 2 \
      -o "$body" -w '%{http_code}\t%{content_type}' "$url" </dev/null); then
      log "$path: fetch failed"
      rejected=$((rejected + 1))
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$path" "$spec" "$url" "$sha" "$bytes" "$fetched" >>"$tmp/manifest"
      continue
    fi
    code=$(printf '%s' "$meta" | cut -f1)
    ct=$(printf '%s' "$meta" | cut -f2 | cut -d';' -f1)
    if [[ "$code" != "200" || "$ct" != "text/plain" || ! -s "$body" ]]; then
      log "$path: rejected (HTTP $code, $ct)"
      rejected=$((rejected + 1))
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$path" "$spec" "$url" "$sha" "$bytes" "$fetched" >>"$tmp/manifest"
      continue
    fi

    new_sha=$(sha256_of "$body")
    new_bytes=$(size_of "$body")
    if [[ "$new_sha" == "$sha" && -f "$root/$path" ]]; then
      unchanged=$((unchanged + 1))
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$path" "$spec" "$url" "$sha" "$bytes" "$fetched" >>"$tmp/manifest"
      continue
    fi

    updated=$((updated + 1))
    if [[ "$mode" == "check" ]]; then
      log "$path: origin changed ($url)"
      continue
    fi
    mkdir -p "$(dirname "$root/$path")"
    cp "$body" "$root/$path"
    log "$path: updated ($new_bytes bytes)"
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$path" "$spec" "$url" "$new_sha" "$new_bytes" "$today" >>"$tmp/manifest"
  done 3<"$manifest"

  if [[ "$mode" == "check" ]]; then
    log "check: $updated changed upstream, $unchanged current, $rejected rejected"
    ((updated == 0 && rejected == 0))
    return
  fi
  cp "$tmp/manifest" "$manifest"
  log "refresh: $updated updated, $unchanged unchanged, $rejected rejected"
  ((rejected == 0))
}

case "${1:-}" in
"") refresh write ;;
--check) refresh check ;;
--verify) verify ;;
*) die "unknown argument: $1 (expected --check, --verify or nothing)" ;;
esac
