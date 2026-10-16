#!/usr/bin/env python3
"""Negative tests for plan/check.py: each rule must catch a deliberate fault."""

import copy
import re
import tempfile
import unittest
from pathlib import Path

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

    def test_decision_without_needs_human(self):
        self.assert_flags(lambda r, k, i: k["P0-08"]["labels"].remove("needs:human"), "decisions need a person")

    def test_label_description_too_long(self):
        meta = copy.deepcopy(self.meta)
        next(iter(meta["labels"].values()))[0]["description"] = "x" * 101
        errors = check.check(self.reqs, meta, copy.deepcopy(self.issues))
        self.assertTrue(any("100-character limit" in e for e in errors), errors[:5])

    def test_decision_without_adr(self):
        self.assert_flags(lambda r, k, i: k["P0-08"].update(deliverable=["a decision"]), "must name a docs/adr/")

    def test_decision_naming_only_the_adr_directory(self):
        self.assert_flags(lambda r, k, i: k["P0-08"].update(deliverable=["an entry in docs/adr/"]), "must name a docs/adr/")

    def test_decision_naming_the_template(self):
        self.assert_flags(lambda r, k, i: k["P0-08"].update(deliverable=["docs/adr/0000-template.md"]), "must name a docs/adr/")

    def test_decision_adr_file_patterns(self):
        for ok in ["docs/adr/0012-ui.md: the answer", "docs/adr/00NN-license-review.md"]:
            self.assertTrue(check.ADR_FILE.search(ok), ok)
        for bad in ["docs/adr/12-ui.md", "docs/adr/0012-UI.md", "docs/adr/0012.md", "docs/adr/README.md"]:
            self.assertFalse(check.ADR_FILE.search(bad), bad)

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


class ScopeRule(unittest.TestCase):
    """docs/scope.md must link every LATER issue (P0-18, MVP-3)."""

    @classmethod
    def setUpClass(cls):
        _, cls.issues = check.load_backlog()
        cls.text = check.SCOPE.read_text()

    def scope_errors(self, text):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "scope.md"
            path.write_text(text)
            return check.check_scope(self.issues, path)

    def test_scope_links_every_later_issue(self):
        self.assertEqual(check.check_scope(self.issues), [])

    def test_later_key_missing(self):
        errors = self.scope_errors(re.sub(r"\[L-04\]\([^)]*\)", "", self.text))
        self.assertEqual(errors, ["L-04: LATER issue not linked from docs/scope.md as [L-04](<issue URL>)"])

    def test_later_key_mentioned_but_not_linked(self):
        errors = self.scope_errors(re.sub(r"\[L-05\]\([^)]*\)", "L-05", self.text))
        self.assertEqual(len(errors), 1)
        self.assertIn("L-05", errors[0])

    def test_key_prefix_does_not_count(self):
        # L-1 must not be satisfied by a link to L-10.
        issues = copy.deepcopy(self.issues) + [{"key": "L-1", "milestone": "LATER"}]
        self.assertEqual(len(check.check_scope(issues)), 1)

    def test_new_later_issue_flagged(self):
        issues = copy.deepcopy(self.issues) + [{"key": "L-99", "milestone": "LATER"}]
        self.assertTrue(any(e.startswith("L-99:") for e in check.check_scope(issues)))

    def test_missing_scope_file(self):
        errors = check.check_scope(self.issues, Path("/nonexistent/scope.md"))
        self.assertEqual(len(errors), 1)
        self.assertIn("missing", errors[0])


class RiskRule(unittest.TestCase):
    """docs/risks.md must match the design doc's risk table (P0-19, OPEN-5)."""

    @classmethod
    def setUpClass(cls):
        cls.meta, _ = check.load_backlog()
        cls.risks = check.RISKS.read_text()
        cls.design = check.DESIGN_DOC.read_text()

    def risk_errors(self, risks=None, design=None):
        with tempfile.TemporaryDirectory() as d:
            rp, dp = Path(d) / "risks.md", Path(d) / "design-doc.md"
            rp.write_text(self.risks if risks is None else risks)
            dp.write_text(self.design if design is None else design)
            return check.check_risks(self.meta, rp, dp)

    def test_register_matches_design_doc(self):
        self.assertEqual(check.check_risks(self.meta), [])

    def test_risk_missing_from_register(self):
        risks = "\n".join(l for l in self.risks.splitlines() if not l.startswith("| R-8 |"))
        self.assertEqual(self.risk_errors(risks),
                         ["docs/risks.md: design-doc risk missing from the register: Small audience"])

    def test_new_design_risk_flagged(self):
        design = self.design.replace("| Small audience |", "| Engine bugs | Crashes | Report upstream |\n| Small audience |")
        self.assertEqual(self.risk_errors(design=design),
                         ["docs/risks.md: design-doc risk missing from the register: Engine bugs"])

    def test_risk_not_in_design_doc(self):
        risks = self.risks.replace("| R-8 | Small audience |", "| R-8 | Tiny audience |")
        errors = self.risk_errors(risks)
        self.assertIn("R-8: risk not in the design doc's risk table: Tiny audience", errors)

    def test_mitigation_drift(self):
        risks = self.risks.replace("Pin versions; keep UI", "Pin versions; keep the UI")
        self.assertEqual(self.risk_errors(risks), ["R-7: impact or mitigation differs from the design doc"])

    def test_owner_must_be_area(self):
        risks = self.risks.replace("| area:agent |", "| nobody |")
        self.assertEqual(self.risk_errors(risks), ["R-9: owner must be an area:* label, got 'nobody'"])

    def test_linked_issue_required(self):
        row = next(l for l in self.risks.splitlines() if l.startswith("| R-2 |"))
        cells = row.split("|")
        cells[6] = " none yet "
        self.assertEqual(self.risk_errors(self.risks.replace(row, "|".join(cells))), ["R-2: no linked issue"])

    def test_unknown_status(self):
        row = next(l for l in self.risks.splitlines() if l.startswith("| R-6 |"))
        errors = self.risk_errors(self.risks.replace(row, row.replace("| Open |", "| Done |")))
        self.assertEqual(len(errors), 1)
        self.assertTrue(errors[0].startswith("R-6: status must be one of"))

    def test_malformed_row(self):
        row = next(l for l in self.risks.splitlines() if l.startswith("| R-1 |"))
        errors = self.risk_errors(self.risks.replace(row, row.rsplit("|", 2)[0] + "|"))
        self.assertTrue(any(e.startswith("docs/risks.md: register row needs 7 cells") for e in errors))

    def test_missing_register_file(self):
        errors = check.check_risks(self.meta, Path("/nonexistent/risks.md"))
        self.assertEqual(len(errors), 1)
        self.assertIn("missing", errors[0])

    def test_design_doc_without_risk_table(self):
        design = self.design.replace("## Risks and open questions", "## Something else")
        errors = self.risk_errors(design=design)
        self.assertEqual(len(errors), 1)
        self.assertIn("no risk table", errors[0])


if __name__ == "__main__":
    unittest.main()
