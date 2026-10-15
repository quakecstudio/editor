#!/usr/bin/env python3
"""Tests for the clean-room guard hook. Run: python3 .claude/hooks/test_clean_room_guard.py"""

import json
import subprocess
import sys
import unittest
from pathlib import Path

HOOK = Path(__file__).with_name("clean_room_guard.py")
sys.path.insert(0, str(HOOK.parent))
import clean_room_guard as guard  # noqa: E402


def call(tool, **inp):
    return {"tool_name": tool, "tool_input": inp}


class Blocks(unittest.TestCase):
    def test_blocks(self):
        for event in [
            call("WebFetch", url="https://github.com/id-Software/Quake/blob/master/WinQuake/pr_exec.c", prompt="x"),
            call("Bash", command="git clone https://github.com/id-Software/Quake-Tools.git"),
            call("Bash", command="gh repo clone quakecstudio/oracles"),
            call("Read", file_path="/Users/j/src/oracles/idvm/pr_exec.c"),
            call("Grep", pattern="OP_STATE", path="../oracles/"),
            call("WebFetch", url="https://github.com/fte-team/fteqw/blob/master/engine/qclib/qcc_pr_comp.c", prompt="x"),
            call("WebSearch", query="pr_edict.c ED_ParseEdict source"),
            call("Read", file_path="WinQuake/progdefs.h"),
        ]:
            with self.subTest(event=event):
                self.assertIsNotNone(guard.verdict(event))

    def test_allows(self):
        for event in [
            call("Read", file_path="docs/spec/vm.md"),
            call("Grep", pattern="FTEQCC", path="docs/"),
            call("Bash", command="cargo test -p qs-vm"),
            call("WebFetch", url="https://github.com/id-Software/quake-rerelease-qc/blob/main/quakec/weapons.qc", prompt="x"),
            call("WebFetch", url="https://docs.rs/rowan/latest/rowan/", prompt="x"),
            call("Read", file_path="crates/qs-progs/src/progs.rs"),
            call("Bash", command="curl -LO https://github.com/quakecstudio/oracles/releases/download/v1/idvm-linux-x64.tar.gz"),
            call("Bash", command="target/oracle-bin/idvm --trace corpus/id101/progs.dat"),
        ]:
            with self.subTest(event=event):
                self.assertIsNone(guard.verdict(event))

    def test_exit_codes(self):
        bad = json.dumps(call("Bash", command="git clone https://github.com/id-Software/Quake"))
        good = json.dumps(call("Bash", command="ls docs/spec"))
        run = lambda s: subprocess.run([sys.executable, str(HOOK)], input=s, text=True, capture_output=True)
        self.assertEqual(run(bad).returncode, 2)
        self.assertIn("clean-room guard", run(bad).stderr)
        self.assertEqual(run(good).returncode, 0)


if __name__ == "__main__":
    unittest.main()
