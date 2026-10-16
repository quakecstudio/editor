#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Kiro SessionStart fallback: make PyYAML importable for everything in plan/ when the
# authoritative setup (.claude/hooks/session-setup.sh, run on agentSpawn) could not install
# it because HOME and the system site-packages are read-only, as in some sandboxed sessions.
#
# This does NOT replace session-setup.sh; it is a narrow fallback for the one dependency the
# plan checks need (AGENTS.md lists "Python 3 with PyYAML"). It installs into a writable,
# workspace-local directory and prints the PYTHONPATH to use, so it never writes outside the
# workspace. It is idempotent, skips when yaml already imports, and always exits 0 so it can
# never fail the session.

set -uo pipefail

# Already satisfied by the system or by the authoritative setup: nothing to do.
if python3 -c "import yaml" >/dev/null 2>&1; then
  exit 0
fi

# The workspace root persists across a session; HOME and /tmp may not.
root=$(cd "$(dirname "$0")/../.." && pwd)
target="$root/.agent-pydeps"

if ! PYTHONPATH="$target" python3 -c "import yaml" >/dev/null 2>&1; then
  mkdir -p "$target" 2>/dev/null || exit 0
  # --target keeps it out of read-only HOME; the warning about the root user is harmless here.
  python3 -m pip install --quiet --disable-pip-version-check --root-user-action=ignore \
    --target "$target" pyyaml >/dev/null 2>&1 || true
fi

# Confirm and tell the session how to pick it up. SessionStart stdout is forwarded to the
# model, so this line tells the agent to prepend the directory when it runs plan/ scripts.
if PYTHONPATH="$target" python3 -c "import yaml" >/dev/null 2>&1; then
  echo "PyYAML is available for the plan checks via PYTHONPATH=$target (HOME is read-only," \
    "so it was installed into the workspace). Run plan/ scripts with that PYTHONPATH set," \
    "e.g. PYTHONPATH=$target python3 plan/check.py."
fi

exit 0
