#!/usr/bin/env python3
"""Tests for plan/status.py on a small synthetic plan."""

import unittest

import status


def issue(key, milestone="P1", labels=("type:feature",), depends=()):
    return {"key": key, "milestone": milestone, "title": key, "labels": list(labels),
            "depends": list(depends)}


GATE0 = issue("P0-20", "P0", ("type:gate", "needs:human"))
SPEC = issue("P1-01", labels=("type:spec",), depends=["P0-20"])
IMPL = issue("P1-07", labels=("type:feature",), depends=["P0-20", "P1-01"])
TOOL = issue("P1-14", labels=("type:chore",), depends=["P0-20"])
NEXT = issue("P1-08", labels=("type:feature",), depends=["P0-20", "P1-07"])
REVIEW = issue("P1-R0", labels=("type:readiness",), depends=["P0-20", "P1-07", "P1-08"])
ABOVE = issue("P1-30", labels=("type:feature",), depends=["P0-20", "P1-R0"])
GATE1 = issue("P1-25", labels=("type:gate", "needs:human"),
              depends=["P0-20", "P1-01", "P1-07", "P1-14", "P1-08", "P1-R0", "P1-30"])
PLAN = [GATE0, SPEC, IMPL, TOOL, NEXT, REVIEW, ABOVE, GATE1]


def states(closed=(), numbers=None):
    numbers = numbers or {}
    return {i["key"]: {"number": numbers.get(i["key"]),
                       "state": "closed" if i["key"] in closed else "open"} for i in PLAN}


class Tracks(unittest.TestCase):
    def test_track_of(self):
        self.assertEqual(status.track_of(SPEC), "build")
        self.assertEqual(status.track_of(IMPL), "build")
        self.assertEqual(status.track_of(REVIEW), "review")
        self.assertEqual(status.track_of(GATE1), "human")
        self.assertEqual(status.track_of(issue("X", labels=("type:feature", "c-tools"))), "build")


class Verdicts(unittest.TestCase):
    def test_waiting_on_previous_gate(self):
        r = status.compute(PLAN, states(numbers={"P0-20": 20}), "P1", "agents")
        self.assertEqual(r["verdict"], "WAITING ON PEOPLE #20")

    def test_in_progress_lists_ready_issues(self):
        r = status.compute(PLAN, states(closed={"P0-20"}), "P1", "agents")
        self.assertEqual([x["key"] for x in r["ready"]], ["P1-01", "P1-14"])
        self.assertTrue(r["verdict"].startswith("IN PROGRESS"))

    def test_build_track_blocked_by_review_track(self):
        done = {"P0-20", "P1-01", "P1-07", "P1-14", "P1-08"}
        r = status.compute(PLAN, states(closed=done), "P1", "build")
        self.assertEqual(r["verdict"], "BLOCKED P1-R0")

    def test_chain_inside_track_is_followed(self):
        r = status.compute(PLAN, states(closed={"P0-20", "P1-01", "P1-14"}), "P1", "review")
        rev = next(w for w in r["waiting"] if w["key"] == "P1-R0")
        self.assertEqual([b["key"] for b in rev["blocked_by"]], ["P1-07", "P1-08"])

    def test_agents_complete_while_gate_open(self):
        done = {"P0-20", "P1-01", "P1-07", "P1-14", "P1-08", "P1-R0", "P1-30"}
        r = status.compute(PLAN, states(closed=done), "P1", "agents")
        self.assertEqual(r["verdict"], "COMPLETE")
        r = status.compute(PLAN, states(closed=done), "P1", "all")
        self.assertTrue(r["verdict"].startswith("IN PROGRESS, next"))

    def test_status_line(self):
        r = status.compute(PLAN, states(closed={"P0-20"}, numbers={"P1-01": 33, "P1-14": 46}), "P1", "agents")
        self.assertEqual(r["status_line"], "STATUS P1 agents: IN PROGRESS, next #33 #46")


class RealPlan(unittest.TestCase):
    def test_runs_on_the_real_plan(self):
        _, issues = status.load_backlog()
        r = status.compute(issues, {}, "P0", "agents")
        self.assertTrue(r["verdict"].startswith("IN PROGRESS"))
        self.assertIn("P0-01", [x["key"] for x in r["ready"]])


if __name__ == "__main__":
    unittest.main()
