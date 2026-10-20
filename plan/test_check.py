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


def first_key(issues: list[dict], what: str, pred) -> str:
    """The key of the first issue `pred` accepts, so a test names an issue by its role in the
    plan rather than by a key the plan may change."""
    for issue in issues:
        if pred(issue):
            return issue["key"]
    raise AssertionError(f"the plan has no {what}")


def labelled(label: str):
    return lambda issue: label in issue.get("labels", [])


class CheckRules(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.reqs = check.load_requirements()
        cls.meta, cls.issues = check.load_backlog()
        issues = cls.issues
        by_key = {i["key"]: i for i in issues}
        cls.some = issues[0]["key"]
        cls.earlier = first_key(issues, f"issue in {check.PHASE_ORDER[1]}",
                                lambda i: i["milestone"] == check.PHASE_ORDER[1])
        cls.later = first_key(issues, f"issue in {check.PHASE_ORDER[2]}",
                              lambda i: i["milestone"] == check.PHASE_ORDER[2])
        depended = {d for i in issues for d in i.get("depends", [])}
        # An issue with dependencies that others depend on, so both sides of the phase
        # comparison meet it.
        cls.middle = first_key(issues, "issue in the middle of a dependency chain",
                               lambda i: i.get("depends") and i["key"] in depended
                               and i["milestone"] in check.PHASE_ORDER)
        cls.repo = first_key(issues, "issue in the repo layer",
                             lambda i: [l for l in i["labels"] if l in check.LAYER_RANK or l == "repo"] == ["repo"])
        cls.dependent = first_key(issues, "issue with a dependency", lambda i: i.get("depends"))
        cls.dependency = by_key[cls.dependent]["depends"][0]
        cls.gate = first_key(issues, "gate", labelled("type:gate"))
        cls.gated = next(d for d in by_key[cls.gate]["depends"]
                         if by_key[d]["milestone"] == by_key[cls.gate]["milestone"])
        cls.bug = first_key(issues, "bug", labelled("type:bug"))
        cls.decision = first_key(issues, "decision", labelled("type:decision"))

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
        self.assert_flags(lambda r, k, i: k[self.some].update(acceptance=[]), f"{self.some}: missing acceptance")

    def test_missing_deliverable(self):
        self.assert_flags(lambda r, k, i: k[self.some].update(deliverable=[]), f"{self.some}: missing deliverable")

    def test_missing_verification(self):
        self.assert_flags(lambda r, k, i: k[self.some].pop("verify"), f"{self.some}: missing verify")

    def test_unknown_requirement(self):
        self.assert_flags(lambda r, k, i: k[self.some]["reqs"].append("NOPE-1"),
                          f"{self.some}: unknown requirement NOPE-1")

    def test_later_phase_dependency(self):
        self.assert_flags(lambda r, k, i: k[self.earlier].setdefault("depends", []).append(self.later),
                          "from a later phase")

    def test_unknown_milestone_is_a_problem_not_an_exception(self):
        for milestone in ("P9", "Phase 1", None):
            self.assert_flags(lambda r, k, i, m=milestone: k[self.middle].update(milestone=m),
                              f"{self.middle}: unknown milestone {milestone}")

    def test_layer_label(self):
        self.assert_flags(lambda r, k, i: k[self.repo]["labels"].remove("repo"),
                          f"{self.repo}: needs exactly one layer label, has []")
        self.assert_flags(lambda r, k, i: k[self.repo]["labels"].append("L1-vm"),
                          f"{self.repo}: needs exactly one layer label, has ['repo', 'L1-vm']")

    def test_cycle(self):
        self.assert_flags(lambda r, k, i: k[self.dependency].setdefault("depends", []).append(self.dependent),
                          "dependency cycle")

    def test_gate_incomplete(self):
        self.assert_flags(lambda r, k, i: k[self.gate]["depends"].remove(self.gated), "gate does not depend")

    def test_bug_without_template_fields(self):
        self.assert_flags(lambda r, k, i: k[self.bug].pop("summary"), f"{self.bug}: a bug needs summary")
        self.assert_flags(lambda r, k, i: k[self.bug].pop("environment"), f"{self.bug}: a bug needs environment")

    def test_decision_without_needs_human(self):
        self.assert_flags(lambda r, k, i: k[self.decision]["labels"].remove("needs:human"), "decisions need a person")

    def test_label_description_too_long(self):
        meta = copy.deepcopy(self.meta)
        next(iter(meta["labels"].values()))[0]["description"] = "x" * 101
        errors = check.check(self.reqs, meta, copy.deepcopy(self.issues))
        self.assertTrue(any("100-character limit" in e for e in errors), errors[:5])

    def test_decision_without_adr(self):
        self.assert_flags(lambda r, k, i: k[self.decision].update(deliverable=["a decision"]), "must name a docs/adr/")

    def test_decision_naming_only_the_adr_directory(self):
        self.assert_flags(lambda r, k, i: k[self.decision].update(deliverable=["an entry in docs/adr/"]),
                          "must name a docs/adr/")

    def test_decision_naming_the_template(self):
        self.assert_flags(lambda r, k, i: k[self.decision].update(deliverable=["docs/adr/0000-template.md"]),
                          "must name a docs/adr/")

    def test_decision_adr_file_patterns(self):
        for ok in ["docs/adr/0012-ui.md: the answer", "docs/adr/00NN-license-review.md"]:
            self.assertTrue(check.ADR_FILE.search(ok), ok)
        for bad in ["docs/adr/12-ui.md", "docs/adr/0012-UI.md", "docs/adr/0012.md", "docs/adr/README.md"]:
            self.assertFalse(check.ADR_FILE.search(bad), bad)


def built(key: str, layer: str, depends: tuple[str, ...] = (), kind: str = "type:feature") -> dict:
    """A minimal issue of `kind` in `layer` and phase P1, for the readiness rule's tests."""
    return {"key": key, "milestone": "P1", "labels": [kind, layer], "depends": list(depends)}


class ReadinessRule(unittest.TestCase):
    """Building on a lower layer waits for that layer's readiness (docs/testing.md)."""

    def test_readiness_injected(self):
        issues = [built("D", "L0-formats"), built("R", "L0-formats", kind="type:readiness"),
                  built("X", "L1-vm", ("D",)), built("S", "L1-vm", ("X",)),
                  built("G", "repo", kind="type:gate")]
        check.add_readiness_dependencies(issues)
        by_key = {i["key"]: i for i in issues}
        # X builds on a lower layer and waits for its readiness; S builds on its own layer.
        self.assertEqual(by_key["X"]["depends"], ["D", "R"])
        self.assertEqual(by_key["S"]["depends"], ["X"])
        # The phase's gate waits for every readiness issue in the phase.
        self.assertEqual(by_key["G"]["depends"], ["R"])

    def test_missing_readiness_flagged(self):
        issues = [built("D", "L0-formats"), built("X", "L1-vm", ("D",))]
        check.add_readiness_dependencies(issues)
        errors = issues[1].get("_errors", [])
        self.assertTrue(any("no readiness issue exists" in e for e in errors), errors)


class ScopeRule(unittest.TestCase):
    """docs/scope.md must link every LATER issue (P0-18, MVP-3)."""

    @classmethod
    def setUpClass(cls):
        _, cls.issues = check.load_backlog()
        cls.text = check.SCOPE.read_text()
        later = [i["key"] for i in cls.issues if i["milestone"] == "LATER"]
        if len(later) < 2:
            raise AssertionError(f"the plan needs two LATER issues for these tests: {later}")
        cls.unlinked, cls.mentioned = later[0], later[1]

    def scope_errors(self, text):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "scope.md"
            path.write_text(text)
            return check.check_scope(self.issues, path)

    def test_scope_links_every_later_issue(self):
        self.assertEqual(check.check_scope(self.issues), [])

    def test_later_key_missing(self):
        key = self.unlinked
        errors = self.scope_errors(re.sub(rf"\[{re.escape(key)}\]\([^)]*\)", "", self.text))
        self.assertEqual(errors, [f"{key}: LATER issue not linked from docs/scope.md as [{key}](<issue URL>)"])

    def test_later_key_mentioned_but_not_linked(self):
        key = self.mentioned
        errors = self.scope_errors(re.sub(rf"\[{re.escape(key)}\]\([^)]*\)", key, self.text))
        self.assertEqual(len(errors), 1)
        self.assertIn(key, errors[0])

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
        # The register's first row, which the tests change one cell of: id, risk, impact,
        # mitigation, owner, linked issues, status.
        cls.row = next(l for l in cls.risks.splitlines() if l.startswith("| R-"))
        cls.cells = [c.strip() for c in cls.row.strip().strip("|").split("|")]
        cls.rid, cls.title = cls.cells[0], cls.cells[1]

    def risk_errors(self, risks=None, design=None):
        with tempfile.TemporaryDirectory() as d:
            rp, dp = Path(d) / "risks.md", Path(d) / "design-doc.md"
            rp.write_text(self.risks if risks is None else risks)
            dp.write_text(self.design if design is None else design)
            return check.check_risks(self.meta, rp, dp)

    def with_cell(self, index: int, value: str) -> str:
        """The register with cell `index` of the first row set to `value`."""
        cells = list(self.cells)
        cells[index] = value
        return self.risks.replace(self.row, "| " + " | ".join(cells) + " |")

    def test_register_matches_design_doc(self):
        self.assertEqual(check.check_risks(self.meta), [])

    def test_risk_missing_from_register(self):
        risks = "\n".join(l for l in self.risks.splitlines() if l != self.row)
        self.assertEqual(self.risk_errors(risks),
                         [f"docs/risks.md: design-doc risk missing from the register: {self.title}"])

    def test_new_design_risk_flagged(self):
        design = self.design.replace(f"| {self.title} |",
                                     f"| Engine bugs | Crashes | Report upstream |\n| {self.title} |", 1)
        self.assertEqual(self.risk_errors(design=design),
                         ["docs/risks.md: design-doc risk missing from the register: Engine bugs"])

    def test_risk_not_in_design_doc(self):
        errors = self.risk_errors(self.with_cell(1, "Tiny audience"))
        self.assertIn(f"{self.rid}: risk not in the design doc's risk table: Tiny audience", errors)

    def test_mitigation_drift(self):
        errors = self.risk_errors(self.with_cell(3, self.cells[3] + " only"))
        self.assertEqual(errors, [f"{self.rid}: impact or mitigation differs from the design doc"])

    def test_owner_must_be_area(self):
        errors = self.risk_errors(self.with_cell(4, "nobody"))
        self.assertEqual(errors, [f"{self.rid}: owner must be an area:* label, got 'nobody'"])

    def test_linked_issue_required(self):
        self.assertEqual(self.risk_errors(self.with_cell(5, "none yet")), [f"{self.rid}: no linked issue"])

    def test_unknown_status(self):
        errors = self.risk_errors(self.with_cell(6, "Done"))
        self.assertEqual(len(errors), 1)
        self.assertTrue(errors[0].startswith(f"{self.rid}: status must be one of"))

    def test_malformed_row(self):
        errors = self.risk_errors(self.risks.replace(self.row, "| " + " | ".join(self.cells[:6]) + " |"))
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
        first = next(iter(check.load_requirements()))
        stale = self.expected.replace(f"| {first} |", f"| {first}x |")
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
