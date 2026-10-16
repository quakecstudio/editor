#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Negative tests for plan/check.py: each rule must catch a deliberate fault."""

from __future__ import annotations

import copy
import hashlib
import re
import tempfile
import unittest
from pathlib import Path

import yaml

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

    def test_missing_deliverable(self):
        self.assert_flags(lambda r, k, i: k["P0-01"].update(deliverable=[]), "P0-01: missing deliverable")

    def test_missing_verification(self):
        self.assert_flags(lambda r, k, i: k["P0-01"].pop("verify"), "P0-01: missing verify")

    def test_unknown_requirement(self):
        self.assert_flags(lambda r, k, i: k["P0-01"]["reqs"].append("NOPE-1"), "P0-01: unknown requirement NOPE-1")

    def test_later_phase_dependency(self):
        self.assert_flags(lambda r, k, i: k["P1-10"]["depends"].append("P2-06"), "from a later phase")

    def test_layer_label(self):
        self.assert_flags(lambda r, k, i: k["P0-06"]["labels"].remove("repo"),
                          "P0-06: needs exactly one layer label, has []")
        self.assert_flags(lambda r, k, i: k["P0-06"]["labels"].append("L1-vm"),
                          "P0-06: needs exactly one layer label, has ['repo', 'L1-vm']")

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
        errors = self.scope_errors(re.sub(r"\[L-06\]\([^)]*\)", "L-06", self.text))
        self.assertEqual(len(errors), 1)
        self.assertIn("L-06", errors[0])

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
        risks = self.risks.replace("`muda` menus; P2-17", "`muda` menus only; P2-17")
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
        row = next(l for l in self.risks.splitlines() if l.startswith("| R-7 |"))
        errors = self.risk_errors(self.risks.replace(row, row.replace("| Open |", "| Done |")))
        self.assertEqual(len(errors), 1)
        self.assertTrue(errors[0].startswith("R-7: status must be one of"))

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


class DesignHashRule(unittest.TestCase):
    """A design-doc change fails the check until requirements.yaml records the new hash (P0-05, PM-6)."""

    def hash_errors(self, design: bytes, recorded: str | None):
        with tempfile.TemporaryDirectory() as d:
            dp, rp = Path(d) / "design-doc.md", Path(d) / "requirements.yaml"
            dp.write_bytes(design)
            header = "" if recorded is None else f"{check.DESIGN_HASH_KEY}: {recorded}\n"
            rp.write_text(header + "sections: []\n")
            return check.check_design_hash(rp, dp)

    def test_recorded_hash_matches(self):
        self.assertEqual(check.check_design_hash(), [])

    def test_matching_fixture_passes(self):
        self.assertEqual(self.hash_errors(b"# Design\n", hashlib.sha256(b"# Design\n").hexdigest()), [])

    def test_changed_design_doc_flagged(self):
        recorded = hashlib.sha256(b"# Design\n").hexdigest()
        new = hashlib.sha256(b"# Design v2\n").hexdigest()
        errors = self.hash_errors(b"# Design v2\n", recorded)
        self.assertEqual(len(errors), 1)
        self.assertIn("changed since requirements.yaml was last reviewed", errors[0])
        self.assertTrue(errors[0].endswith(f"design_doc_sha256: {new}"), errors[0])

    def test_missing_hash_flagged(self):
        errors = self.hash_errors(b"# Design\n", None)
        self.assertEqual(len(errors), 1)
        self.assertIn("records no design_doc_sha256", errors[0])

    def test_crlf_checkout_hashes_like_lf(self):
        self.assertEqual(self.hash_errors(b"# Design\r\nText\r\n", hashlib.sha256(b"# Design\nText\n").hexdigest()), [])

    def test_missing_design_doc(self):
        with tempfile.TemporaryDirectory() as d:
            errors = check.check_design_hash(check.REQUIREMENTS, Path(d) / "design-doc.md")
        self.assertEqual(len(errors), 1)
        self.assertIn("missing", errors[0])


class CoverageRule(unittest.TestCase):
    """plan/coverage.md is regenerated with the plan, never left stale (P0-05)."""

    @classmethod
    def setUpClass(cls):
        reqs = check.load_requirements()
        meta, issues = check.load_backlog()
        cls.expected = check.coverage_md(reqs, meta, issues)

    def test_committed_coverage_is_current(self):
        self.assertEqual(check.check_coverage(self.expected), [])

    def test_every_requirement_is_listed(self):
        for r in check.load_requirements():
            self.assertIn(f"| {r} |", self.expected)

    def coverage_errors(self, text):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "coverage.md"
            if text is not None:
                path.write_text(text)
            return check.check_coverage(self.expected, path)

    def test_stale_coverage_flagged(self):
        stale = self.expected.replace("| GOAL-1 |", "| GOAL-1x |")
        self.assertEqual(self.coverage_errors(stale),
                         ["coverage.md: out of date; run python3 plan/check.py --write-coverage"])

    def test_missing_coverage_flagged(self):
        self.assertEqual(len(self.coverage_errors(None)), 1)


class TemplateRule(unittest.TestCase):
    """Issue templates ask for deliverable, acceptance, verification and requirement IDs; the
    PR template carries the definition of done (P0-04, PM-5)."""

    @classmethod
    def setUpClass(cls):
        cls.meta, _ = check.load_backlog()

    def template_errors(self, edit_name=None, edit=None, pr_edit=None):
        with tempfile.TemporaryDirectory() as d:
            templates = Path(d) / "ISSUE_TEMPLATE"
            templates.mkdir()
            for name in check.TEMPLATE_NAMES:
                text = (check.ISSUE_TEMPLATES / f"{name}.yml").read_text()
                if name == edit_name:
                    if edit is None:
                        continue
                    text = edit(text)
                (templates / f"{name}.yml").write_text(text)
            pr = Path(d) / "pull_request_template.md"
            text = check.PR_TEMPLATE.read_text()
            pr.write_text(pr_edit(text) if pr_edit else text)
            return check.check_templates(self.meta, templates, pr)

    def test_templates_pass(self):
        self.assertEqual(check.check_templates(self.meta), [])
        self.assertEqual(self.template_errors(), [])

    def test_every_template_shows_the_required_sections(self):
        for name in check.TEMPLATE_NAMES:
            form = yaml.safe_load((check.ISSUE_TEMPLATES / f"{name}.yml").read_text())
            labels = {f["id"]: f["attributes"]["label"] for f in form["body"] if "id" in f}
            with self.subTest(template=name):
                self.assertEqual(labels["deliverable"], "Deliverable")
                self.assertEqual(labels["acceptance"], "Acceptance criteria")
                self.assertEqual(labels["verification"], "Verification")
                self.assertEqual(labels["requirements"], "Requirement IDs")

    def test_missing_template(self):
        self.assertEqual(self.template_errors("spike"), ["spike.yml: issue template missing"])

    def test_missing_field(self):
        errors = self.template_errors("bug", lambda t: t.replace("id: verification", "id: checks"))
        self.assertEqual(errors, ["bug.yml: no verification field"])

    def test_optional_field(self):
        def make_deliverable_optional(text):
            head, tail = text.split("id: deliverable", 1)
            return head + "id: deliverable" + tail.replace("required: true", "required: false", 1)

        errors = self.template_errors("feature", make_deliverable_optional)
        self.assertEqual(errors, ["feature.yml: the deliverable field is not required"])

    def test_type_label(self):
        errors = self.template_errors("spec", lambda t: t.replace('["type:spec"]', '["type:nope"]'))
        self.assertEqual(errors, ["spec.yml: needs exactly one known type:* label, has ['type:nope']"])
        errors = self.template_errors("spec", lambda t: t.replace('["type:spec"]', "[]"))
        self.assertEqual(errors, ["spec.yml: needs exactly one known type:* label, has []"])

    def test_definition_of_done_drift(self):
        item = self.meta["definition_of_done"][0]
        errors = self.template_errors(pr_edit=lambda t: t.replace(f"- [ ] {item}\n", ""))
        self.assertEqual(errors, [f"pull_request_template.md: definition-of-done item missing: {item}"])

    def test_pr_template_must_ask_about_the_design_doc(self):
        errors = self.template_errors(pr_edit=lambda t: t.replace("design_doc_sha256", "the hash"))
        self.assertEqual(len(errors), 1)
        self.assertIn("does not ask whether requirements.yaml was reviewed", errors[0])

    def test_missing_pr_template(self):
        errors = check.check_templates(self.meta, check.ISSUE_TEMPLATES, Path("/nonexistent/pr.md"))
        self.assertEqual(errors, ["pr.md: pull request template missing"])


if __name__ == "__main__":
    unittest.main()
