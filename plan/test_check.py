#!/usr/bin/env python3
"""Negative tests for plan/check.py: each rule must catch a deliberate fault."""

import copy
import unittest

import check


class CheckRules(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.reqs = check.load_requirements()
        cls.meta, cls.issues = check.load_backlog()

    def run_with(self, mutate):
        reqs, issues = copy.deepcopy(self.reqs), copy.deepcopy(self.issues)
        by_key = {i["key"]: i for i in issues}
        mutate(reqs, by_key, issues)
        return check.check(reqs, self.meta, issues)

    def assert_flags(self, mutate, needle):
        errors = self.run_with(mutate)
        self.assertTrue(any(needle in e for e in errors), f"expected '{needle}' in {errors[:5]}")

    def test_plan_passes(self):
        self.assertEqual(check.check(self.reqs, self.meta, self.issues), [])

    def test_uncovered_requirement(self):
        self.assert_flags(lambda r, k, i: r.update({"X-1": {"id": "X-1", "kind": "stated", "text": "t", "section": "s"}}),
                          "X-1: requirement not covered")

    def test_missing_acceptance(self):
        self.assert_flags(lambda r, k, i: k["P0-01"].update(acceptance=[]), "P0-01: missing acceptance")

    def test_later_phase_dependency(self):
        self.assert_flags(lambda r, k, i: k["P1-10"]["depends"].append("P2-06"), "from a later phase")

    def test_cycle(self):
        self.assert_flags(lambda r, k, i: k["P0-01"].setdefault("depends", []).append("P0-02"), "dependency cycle")

    def test_gate_incomplete(self):
        self.assert_flags(lambda r, k, i: k["P1-25"]["depends"].remove("P1-15"), "gate does not depend")

    def test_clean_room_without_spec(self):
        def m(r, k, i):
            k["P1-08"]["depends"] = ["P0-20"]
            k["P1-07"]["depends"] = ["P0-20"]
        self.assert_flags(m, "P1-08: clean-room:impl issue has no spec issue upstream")

    def test_decision_without_adr(self):
        self.assert_flags(lambda r, k, i: k["P0-08"].update(deliverable=["a decision"]), "must name a docs/adr/")

    def test_readiness_injected(self):
        # qc-edict (L1) builds on qc-progs (L0), so it must wait for L0 readiness in Phase 1.
        self.assertIn("P1-R0", {i["key"]: i for i in self.issues}["P1-09"]["depends"])

    def test_missing_readiness_flagged(self):
        reqs = copy.deepcopy(self.reqs)
        meta, issues = check.load_backlog()
        issues = [i for i in issues if i["key"] != "P1-R0"]
        for i in issues:
            i["depends"] = [d for d in i.get("depends", []) if d != "P1-R0"]
            i.pop("_errors", None)
        check.add_readiness_dependencies(issues)
        errors = check.check(reqs, meta, issues)
        self.assertTrue(any("no readiness issue exists" in e for e in errors))


if __name__ == "__main__":
    unittest.main()
