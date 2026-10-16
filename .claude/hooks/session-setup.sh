#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Claude Code SessionStart hook: prepare a fresh Linux container (Claude Code on the web,
# other cloud sessions) to build and check this repository. AGENTS.md describes it.
#
# Every step is idempotent and skips when already satisfied or when the network policy
# blocks a download. The hook must never fail the session, so it always exits 0, and it
# does nothing outside Linux: developer machines are set up by hand (CONTRIBUTING.md).
# For the same reason it runs without `set -e`: a failed step must not stop the next one.

set -uo pipefail

[[ "$(uname)" == "Linux" ]] || exit 0

as_root() {
  if [[ "$(id -u)" == "0" ]]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  else
    return 1
  fi
}

# System packages: a C toolchain for build scripts, PyYAML for plan/, and the shell linters
# the pre-commit hooks run. dpkg -s fails if any one is missing.
if command -v apt-get >/dev/null 2>&1; then
  pkgs=(build-essential pkg-config curl jq python3 python3-pip python3-yaml shellcheck shfmt)
  if ! dpkg -s "${pkgs[@]}" >/dev/null 2>&1; then
    # Not chained with &&: one broken third-party repository makes `update` fail, but
    # installs from the indexes already cached still succeed.
    as_root apt-get update -qq >/dev/null 2>&1
    as_root apt-get install -y -qq "${pkgs[@]}" >/dev/null 2>&1
  fi
fi

# rustup only; rust-toolchain.toml pins the toolchain, which cargo installs on first use.
if ! command -v rustup >/dev/null 2>&1 && [[ ! -x "$HOME/.cargo/bin/rustup" ]]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --default-toolchain none --profile minimal >/dev/null 2>&1
fi

# prek runs the pre-commit hooks. PyPI ships prebuilt wheels and stays reachable when
# github.com downloads are restricted.
if ! command -v prek >/dev/null 2>&1 && [[ ! -x "$HOME/.local/bin/prek" ]]; then
  python3 -m pip install --user --quiet prek >/dev/null 2>&1 ||
    python3 -m pip install --user --quiet --break-system-packages prek >/dev/null 2>&1
fi

# Install the git hooks once the repository has a hook configuration (#2).
root=$(cd "$(dirname "$0")/../.." && pwd)
if [[ -f "$root/.pre-commit-config.yaml" ]]; then
  prek_bin=$(command -v prek || printf '%s' "$HOME/.local/bin/prek")
  if [[ -x "$prek_bin" ]]; then
    (cd "$root" && "$prek_bin" install --hook-type pre-commit --hook-type pre-push >/dev/null 2>&1)
  fi
fi

exit 0
