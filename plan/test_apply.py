#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for plan/apply.py against an in-memory GitHub, using the real plan."""

import contextlib
import io
import re
import sys
import unittest
from unittest import mock

import apply
from check import load_backlog

REPO = "repos/quakecstudio/editor"
OPERATION = re.compile(r"(?:query|mutation) (\w+)")


class FakeGitHub:
    """Enough of the REST and GraphQL APIs for apply.py; records every write."""

    def __init__(self):
        self.labels, self.milestones, self.issues = {}, {}, {}
        self.fields, self.views, self.items = {}, {}, {}
        # GitHub's built-in Status field: people and start.sh set it, the plan never does.
        self.fields["Status"] = {"id": "F_status", "name": "Status", "dataType": "SINGLE_SELECT",
                                 "options": [{"id": "O_todo", "name": "Todo"},
                                             {"id": "O_in_progress", "name": "In Progress"},
                                             {"id": "O_done", "name": "Done"}]}
        self.writes = []
        self.ids = 0

    def new_id(self, prefix):
        self.ids += 1
        return f"{prefix}_{self.ids}"

    def __call__(self, args, payload=None):
        if args == ["graphql"]:
            return {"data": self.graphql(OPERATION.search(payload["query"]).group(1), payload["variables"])}
        method = args[1] if args[0] == "-X" else "GET"
        path = args[-1]
        if method == "GET":
            return self.get(path)
        self.writes.append((method, path))
        return self.rest_write(method, path, payload)

    def get(self, path):
        page = int(re.search(r"[?&]page=(\d+)", path).group(1))
        if "/labels" in path:
            rows = list(self.labels.values())
        elif "/milestones" in path:
            rows = list(self.milestones.values())
        else:
            rows = list(self.issues.values())
        return rows[(page - 1) * 100:page * 100]

    def rest_write(self, method, path, body):
        kind, _, ident = path[len(REPO) + 1:].partition("/")
        if kind == "labels":
            self.labels[body["name"]] = dict(body)
            return None
        if kind == "milestones":
            number = len(self.milestones) + 1 if method == "POST" else int(ident)
            self.milestones[number] = {**body, "number": number}
            return self.milestones[number]
        number = len(self.issues) + 1 if method == "POST" else int(ident)
        old = self.issues.get(number, {"node_id": self.new_id("I")})
        self.issues[number] = {
            "number": number, "node_id": old["node_id"], "title": body["title"], "body": body["body"],
            "labels": [{"name": l} for l in body["labels"]],
            "milestone": {"number": body["milestone"]}, "state": old.get("state", "open")}
        return self.issues[number]

    def graphql(self, op, v):
        if op == "ProjectState":
            return {"organization": {"projectV2": self.project_state(v["after"])}}
        self.writes.append((op, None))
        if op == "CreateField":
            f = v["input"]
            fid = self.new_id("F")
            options = [{**o, "id": self.new_id("O")} for o in f.get("singleSelectOptions", [])]
            self.fields[f["name"]] = {"id": fid, "name": f["name"], "dataType": f["dataType"], "options": options}
        elif op == "UpdateField":
            field = next(f for f in self.fields.values() if f["id"] == v["input"]["fieldId"])
            field["options"] = [{**o, "id": o.get("id") or self.new_id("O")}
                                for o in v["input"]["singleSelectOptions"]]
        elif op == "AddItem":
            item = {"id": self.new_id("PVTI"), "values": {}}
            self.items[v["content"]] = item
            return {"addProjectV2ItemById": {"item": {"id": item["id"]}}}
        elif op == "SetValue":
            item = next(i for i in self.items.values() if i["id"] == v["item"])
            field = next(f for f in self.fields.values() if f["id"] == v["field"])
            item["values"][field["name"]] = v["value"].get("text", v["value"].get("singleSelectOptionId"))
        elif op == "CreateView":
            vid = self.new_id("PVTV")
            self.views[v["input"]["name"]] = {"id": vid, "name": v["input"]["name"],
                                              "layout": v["input"]["layout"], "filter": "", "visible": []}
            return {"createProjectV2View": {"projectV2View": {"id": vid}}}
        elif op == "UpdateView":
            u = v["input"]
            view = next(x for x in self.views.values() if x["id"] == u["viewId"])
            view["layout"] = u.get("layout", view["layout"])
            view["filter"] = u.get("filter", view["filter"])
            if "configuration" in u:
                view["visible"] = u["configuration"]["visibleFieldIds"]
        return {}

    def project_state(self, after):
        start = int(after or 0)
        contents = list(self.items)
        page = contents[start:start + 100]

        def value(name, raw):
            key = "text" if self.fields[name]["dataType"] == "TEXT" else "optionId"
            return {key: raw, "field": {"name": name}}

        nodes = [{"id": self.items[c]["id"], "content": {"id": c},
                  "fieldValues": {"nodes": [value(n, r) for n, r in self.items[c]["values"].items()]}}
                 for c in page]
        nodes.append({"id": "PVTI_draft", "content": {}, "fieldValues": {"nodes": []}})
        return {
            "id": "PVT_1",
            "fields": {"nodes": [{}, *self.fields.values()]},
            "views": {"nodes": [{"id": x["id"], "name": x["name"], "layout": x["layout"], "filter": x["filter"],
                                 "fields": {"nodes": [{"id": i} for i in x["visible"]]}}
                                for x in self.views.values()]},
            "items": {"nodes": nodes, "pageInfo": {"hasNextPage": start + 100 < len(contents),
                                                    "endCursor": str(start + 100)}},
        }


def run(fake, *extra):
    out = io.StringIO()
    argv = ["apply.py", "--repo", "quakecstudio/editor", *extra]
    with mock.patch.object(apply, "gh", fake), mock.patch.object(sys, "argv", argv), \
            contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
        code = apply.main()
    return code, out.getvalue()


class Apply(unittest.TestCase):
    def setUp(self):
        self.fake = FakeGitHub()
        self.meta, self.plan = load_backlog()

    def test_second_run_writes_nothing(self):
        code, _ = run(self.fake)
        self.assertEqual(code, 0)
        self.assertTrue(self.fake.writes)
        self.fake.writes.clear()
        code, out = run(self.fake)
        self.assertEqual((code, self.fake.writes), (0, []), out)
        self.assertIn("(0 updated)", out)
        self.assertIn("(0 writes)", out)

    def test_every_issue_carries_its_plan_key_and_project_fields(self):
        run(self.fake)
        keys = sorted(apply.MARKER.search(i["body"]).group(1) for i in self.fake.issues.values())
        self.assertEqual(keys, sorted(i["key"] for i in self.plan))
        self.assertEqual(len(self.fake.items), len(self.plan))
        names = {o["id"]: o["name"] for f in self.fake.fields.values() for o in f["options"]}
        by_node = {i["node_id"]: apply.MARKER.search(i["body"]).group(1) for i in self.fake.issues.values()}
        plan = {i["key"]: i for i in self.plan}
        for node, item in self.fake.items.items():
            want = apply.project_values(plan[by_node[node]], self.meta)
            got = {n: names.get(v, v) for n, v in item["values"].items()}
            self.assertEqual(got, want)

    def test_views_are_created_with_their_filters_and_fields(self):
        run(self.fake)
        ids = {f["id"]: n for n, f in self.fake.fields.items()}
        for view in apply.VIEWS:
            have = self.fake.views[view["name"]]
            self.assertEqual((have["layout"], have["filter"]), (view["layout"], view["filter"]))
            self.assertEqual([ids[i] for i in have["visible"]], list(view["fields"]))

    def test_drift_on_the_board_is_repaired(self):
        run(self.fake)
        item = next(iter(self.fake.items.values()))
        item["values"]["Requirements"] = "edited by hand"
        del item["values"]["Size"]
        self.fake.views["Decisions"]["filter"] = ""
        self.fake.writes.clear()
        run(self.fake)
        self.assertEqual(sorted(w[0] for w in self.fake.writes), ["SetValue", "SetValue", "UpdateView"])

    def test_status_set_on_the_board_is_left_alone(self):
        run(self.fake)
        item = next(iter(self.fake.items.values()))
        item["values"]["Status"] = "O_in_progress"
        del item["values"]["Size"]
        self.fake.writes.clear()
        run(self.fake)
        self.assertEqual([w[0] for w in self.fake.writes], ["SetValue"])
        self.assertEqual(item["values"]["Status"], "O_in_progress")

    def test_changed_options_keep_their_ids(self):
        run(self.fake)
        phase = self.fake.fields["Phase"]
        kept = {o["name"]: o["id"] for o in phase["options"]}
        phase["options"][0]["description"] = "stale"
        self.fake.writes.clear()
        run(self.fake)
        self.assertEqual([w[0] for w in self.fake.writes], ["UpdateField"])
        self.assertEqual({o["name"]: o["id"] for o in phase["options"]}, kept)

    def test_missing_item_is_added_again(self):
        run(self.fake)
        self.fake.items.pop(next(iter(self.fake.items)))
        self.fake.writes.clear()
        run(self.fake)
        self.assertEqual(sorted({w[0] for w in self.fake.writes}), ["AddItem", "SetValue"])
        self.assertEqual(len(self.fake.items), len(self.plan))

    def test_open_issue_whose_key_left_the_plan_is_reported(self):
        run(self.fake)
        self.fake.issues[999] = {"number": 999, "node_id": "I_x", "title": "gone", "labels": [],
                                 "milestone": None, "body": "<!-- plan-key: P9-99 -->", "state": "open"}
        code, out = run(self.fake)
        self.assertEqual(code, 1)
        self.assertIn("#999 P9-99", out)
        self.fake.issues[999]["state"] = "closed"
        code, out = run(self.fake)
        self.assertEqual(code, 0)
        self.assertNotIn("P9-99", out)

    def test_dry_run_touches_nothing(self):
        code, out = run(self.fake, "--dry-run")
        self.assertEqual((code, self.fake.writes), (0, []))
        self.assertIn("fields Phase, Layer, Size, Requirements", out)


if __name__ == "__main__":
    unittest.main()
