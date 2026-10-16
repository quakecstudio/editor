#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Render the plan as a single self-contained HTML review page.

Usage: python3 plan/review_page.py OUT.html
"""

from __future__ import annotations

import json
import sys

from check import PHASE_ORDER, check, load_backlog, load_requirements

DECISIONS = [
    ("Readiness thresholds", "docs/testing.md proposes 90% line and 80% branch coverage, a 90% mutation score, 100% spec-citation coverage, 24 h of clean fuzzing and 20 flake-free runs per OS before any layer is built on. P0-24 records the final numbers."),
    ("FTEQCC as a test-only oracle", "id's qcc cannot compile the 2021 rerelease, so the plan builds a pinned FTEQCC with the oracles to define correct behaviour in CI (P1-26). The IDE itself still ships with no outside compiler. Drop it and the rerelease has no external reference."),
    ("MVP scope", "The design doc's MVP list does not name ACP hosting, Cmd/Ctrl+K, next-edit, tree-sitter, editor extensions, the restart-loop adapters, the KEX target or the update mechanism, but the plan builds them before the MVP. P0-21 asks you to confirm each one."),
    ("panic = \"unwind\"", "vouch aborts on panic. This plan unwinds in release builds so the command guard can keep a session alive after one bad command."),
    ("Config and log locations", "vouch uses XDG directories on every OS, including macOS. Desktop apps usually use ~/Library/Application Support and %APPDATA%. P0-01 decides."),
    ("The editor repo is public", "quakecstudio/editor is public and, since ADR-0002, licensed GPL-3.0-or-later like the rest of the project. Nothing is published as a release until the license compliance review (P4-22)."),
    ("Make main the default branch", "The plan is on main, but GitHub made docs/plan the default because it was pushed first. Change it under Settings > General; this session is not allowed to change repository settings."),
]


def main() -> int:
    reqs = load_requirements()
    meta, issues = load_backlog()
    errors = check(reqs, meta, issues)
    issues.sort(key=lambda i: (PHASE_ORDER.index(i["milestone"]), i["key"]))
    data = {
        "milestones": meta["milestones"],
        "dod": meta["definition_of_done"],
        "sizes": meta["sizes"],
        "issues": [{k: v for k, v in i.items() if not k.startswith("_")} for i in issues],
        "reqs": [{"id": k, **{x: v[x] for x in ("kind", "text", "section")}} for k, v in reqs.items()],
        "decisions": [{"title": t, "text": x} for t, x in DECISIONS],
        "errors": errors,
    }
    html = TEMPLATE.replace("__DATA__", json.dumps(data).replace("</", "<\\/"))
    with open(sys.argv[1], "w") as f:
        f.write(html)
    print(f"wrote {sys.argv[1]}: {len(issues)} issues, {len(reqs)} requirements, {len(errors)} errors")
    return 0


TEMPLATE = r"""<title>QuakeC Studio Backlog</title>
<meta name="claude:viewport-fit-sides" content="cover">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&family=IBM+Plex+Sans+Condensed:wght@500;600;700&family=IBM+Plex+Sans:wght@400;500;600&display=swap">
<style>
/* Layout: a plan reviewer. Summary and decisions first, then phases as a timeline strip, then the filterable issue list; coverage on its own tab. */
:root {
  --bg: #f6f7f8; --surface: #ffffff; --ink: #1b2026; --muted: #5d6773; --line: #dde1e6;
  --accent: #b5651d; --accent-soft: #f6e9dc; --ok: #2f7d4f; --warn: #a06600; --bad: #b3261e;
  --chip: #eef0f3; --gate: #fbe9e7; --ready: #ece8fb; --spec: #e5eff9;
  --display: "IBM Plex Sans Condensed", "Arial Narrow", system-ui, sans-serif;
  --body: "IBM Plex Sans", system-ui, -apple-system, "Segoe UI", sans-serif;
  --mono: "IBM Plex Mono", ui-monospace, "SF Mono", Menlo, monospace;
}
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {
  --bg: #121519; --surface: #1a1e23; --ink: #e6e9ed; --muted: #9aa4af; --line: #2c3239;
  --accent: #e39a52; --accent-soft: #3a2a1b; --ok: #6cc28f; --warn: #e0b04f; --bad: #f08b84;
  --chip: #252a31; --gate: #3a2321; --ready: #2a2640; --spec: #1f2c3a; color-scheme: dark; } }
:root[data-theme="dark"] {
  --bg: #121519; --surface: #1a1e23; --ink: #e6e9ed; --muted: #9aa4af; --line: #2c3239;
  --accent: #e39a52; --accent-soft: #3a2a1b; --ok: #6cc28f; --warn: #e0b04f; --bad: #f08b84;
  --chip: #252a31; --gate: #3a2321; --ready: #2a2640; --spec: #1f2c3a; color-scheme: dark; }
* { box-sizing: border-box; }
body { background: var(--bg); color: var(--ink); font: 15px/1.55 var(--body);
  padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px)); padding-block: 28px 64px; }
.wrap { max-width: 1080px; margin: 0 auto; display: grid; gap: 28px; }
h1, h2, h3 { font-family: var(--display); text-wrap: balance; margin: 0; letter-spacing: .005em; }
h1 { font-size: 34px; font-weight: 700; }
h2 { font-size: 22px; font-weight: 600; }
.lede { color: var(--muted); max-width: 68ch; margin: 6px 0 0; }
.status { display: flex; flex-wrap: wrap; gap: 8px 18px; margin-top: 14px; font-size: 14px; color: var(--muted); }
.status b { color: var(--ink); font-variant-numeric: tabular-nums; }
.pill { display: inline-flex; align-items: center; gap: 6px; padding: 2px 10px; border-radius: 999px; font-size: 13px; font-weight: 600; }
.pill.ok { background: color-mix(in srgb, var(--ok) 15%, transparent); color: var(--ok); }
.pill.bad { background: color-mix(in srgb, var(--bad) 15%, transparent); color: var(--bad); }
.decisions { background: var(--accent-soft); border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent); border-radius: 10px; padding: 18px 20px; display: grid; gap: 12px; }
.decisions ol { margin: 0; padding-left: 22px; display: grid; gap: 10px; }
.decisions li b { display: block; }
.decisions li span { color: var(--muted); }
.tabs { display: flex; gap: 4px; border-bottom: 1px solid var(--line); }
.tabs button { font: 600 14px var(--body); background: none; border: 0; color: var(--muted); padding: 10px 14px; cursor: pointer; border-bottom: 2px solid transparent; margin-bottom: -1px; }
.tabs button[aria-selected="true"] { color: var(--ink); border-bottom-color: var(--accent); }
button:focus-visible, input:focus-visible, select:focus-visible, summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.phases { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 8px; }
.phase { background: var(--surface); border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; cursor: pointer; text-align: left; font: inherit; color: inherit; display: grid; gap: 4px; align-content: start; }
.phase[aria-pressed="true"] { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent) inset; }
.phase .k { font: 600 12px var(--mono); color: var(--accent); }
.phase .t { font: 600 14px/1.3 var(--display); }
.phase .n { font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; }
.gatebox { background: var(--surface); border: 1px solid var(--line); border-left: 3px solid var(--bad); border-radius: 6px; padding: 10px 14px; font-size: 14px; }
.gatebox b { font-family: var(--display); }
.filters { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
.filters input, .filters select { font: 14px var(--body); padding: 7px 10px; border: 1px solid var(--line); border-radius: 6px; background: var(--surface); color: var(--ink); min-width: 0; }
.filters input { flex: 1 1 220px; }
.count { color: var(--muted); font-size: 13px; font-variant-numeric: tabular-nums; }
.list { display: grid; gap: 6px; }
details.issue { background: var(--surface); border: 1px solid var(--line); border-radius: 8px; }
details.issue[data-type="type:gate"] { background: var(--gate); }
details.issue[data-type="type:readiness"] { background: var(--ready); }
details.issue[data-type="type:spec"] { background: var(--spec); }
summary { list-style: none; cursor: pointer; padding: 10px 14px; display: grid; grid-template-columns: 74px 1fr auto; gap: 4px 12px; align-items: baseline; }
summary::-webkit-details-marker { display: none; }
.key { font: 600 13px var(--mono); color: var(--accent); }
.title { font-weight: 500; min-width: 0; }
.size { font: 12px var(--mono); color: var(--muted); }
.chips { grid-column: 2 / 4; display: flex; flex-wrap: wrap; gap: 4px; }
.chip { font: 11.5px var(--mono); background: var(--chip); color: var(--muted); padding: 1px 7px; border-radius: 4px; }
.chip.type { color: var(--ink); font-weight: 600; }
.body { padding: 2px 14px 14px 100px; display: grid; gap: 10px; font-size: 14px; }
.body h4 { margin: 0; font: 600 11.5px var(--body); text-transform: uppercase; letter-spacing: .08em; color: var(--muted); }
.body ul { margin: 2px 0 0; padding-left: 18px; display: grid; gap: 3px; }
.body code, .reqid { font: 12.5px var(--mono); }
.deps { display: flex; flex-wrap: wrap; gap: 4px; }
.deps a { font: 12px var(--mono); color: var(--accent); text-decoration: none; background: var(--chip); padding: 1px 7px; border-radius: 4px; }
.deps a:hover { text-decoration: underline; }
.dod { color: var(--muted); font-size: 13px; }
.cov-wrap { overflow-x: auto; background: var(--surface); border: 1px solid var(--line); border-radius: 8px; }
table { border-collapse: collapse; width: 100%; font-size: 13.5px; }
th, td { text-align: left; padding: 7px 12px; border-bottom: 1px solid var(--line); vertical-align: top; }
th { font: 600 12px var(--body); text-transform: uppercase; letter-spacing: .06em; color: var(--muted); position: sticky; top: 0; background: var(--surface); }
td.sec { font: 600 14px var(--display); background: var(--chip); }
.implied { color: var(--warn); font-weight: 600; }
.errors { color: var(--bad); }
@media (max-width: 640px) {
  summary { grid-template-columns: 1fr auto; }
  summary .key { grid-column: 1 / 3; }
  .chips { grid-column: 1 / 3; }
  .body { padding-left: 14px; }
  h1 { font-size: 28px; }
}
@media (prefers-reduced-motion: reduce) { * { transition: none !important; } }
</style>

<div class="wrap">
  <header>
    <h1>QuakeC Studio Backlog</h1>
    <p class="lede">Every requirement in the design document has an ID, and every issue names its deliverable, acceptance criteria and verification. Phase order and layer test-suite readiness are enforced as dependencies. All issues are on GitHub in quakecstudio/editor, one per plan key.</p>
    <div class="status" id="status"></div>
  </header>

  <section class="decisions" aria-labelledby="dh">
    <h2 id="dh">Decisions for you</h2>
    <ol id="decisions"></ol>
  </section>

  <div>
    <div class="tabs" role="tablist">
      <button role="tab" id="tab-issues" aria-selected="true" aria-controls="pane-issues">Issues</button>
      <button role="tab" id="tab-cov" aria-selected="false" aria-controls="pane-cov">Requirement coverage</button>
      <button role="tab" id="tab-dod" aria-selected="false" aria-controls="pane-dod">Definition of done</button>
    </div>
  </div>

  <section id="pane-issues" role="tabpanel" aria-labelledby="tab-issues" style="display:grid; gap:14px">
    <div class="phases" id="phases"></div>
    <div class="gatebox" id="gatebox" hidden></div>
    <div class="filters">
      <input id="q" type="search" placeholder="Search titles, deliverables, requirement IDs" aria-label="Search">
      <select id="type" aria-label="Type"><option value="">All types</option></select>
      <select id="layer" aria-label="Layer"><option value="">All layers</option></select>
      <span class="count" id="count"></span>
    </div>
    <div class="list" id="list"></div>
  </section>

  <section id="pane-cov" role="tabpanel" aria-labelledby="tab-cov" hidden>
    <div class="cov-wrap"><table><thead><tr><th>ID</th><th>Requirement</th><th>Issues</th></tr></thead><tbody id="cov"></tbody></table></div>
  </section>

  <section id="pane-dod" role="tabpanel" aria-labelledby="tab-dod" hidden>
    <p class="lede">Every issue carries this checklist in addition to its own acceptance criteria.</p>
    <ul id="dod"></ul>
  </section>
</div>

<script>
const D = __DATA__;
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const code = (s) => esc(s).replace(/`([^`]+)`/g, "<code>$1</code>");
const reqText = Object.fromEntries(D.reqs.map((r) => [r.id, r.text]));
const byKey = Object.fromEntries(D.issues.map((i) => [i.key, i]));
const typeOf = (i) => i.labels.find((l) => l.startsWith("type:")) || "";
const layers = ["L0-formats","L1-vm","L2-syntax","L3-semantics","L4-bridges","L5-commands","L6-agent","L7-ui","apps","c-tools","repo"];
const layerOf = (i) => i.labels.find((l) => layers.includes(l)) || "";

const implied = D.reqs.filter((r) => r.kind === "implied").length;
$("#status").innerHTML =
  (D.errors.length ? `<span class="pill bad">${D.errors.length} plan errors</span>` : `<span class="pill ok">Plan check passes</span>`) +
  `<span><b>${D.reqs.length}</b> requirements (${implied} implied)</span>` +
  `<span><b>${D.issues.length}</b> issues</span>` +
  `<span><b>${D.issues.filter((i) => typeOf(i) === "type:readiness").length}</b> readiness reviews</span>` +
  `<span><b>${D.milestones.length}</b> milestones</span>`;
$("#decisions").innerHTML = D.decisions.map((d) => `<li><b>${esc(d.title)}</b><span>${esc(d.text)}</span></li>`).join("");

const types = [...new Set(D.issues.map(typeOf))].sort();
$("#type").innerHTML += types.map((t) => `<option value="${t}">${t.replace("type:", "")}</option>`).join("");
const usedLayers = layers.filter((l) => D.issues.some((i) => layerOf(i) === l));
$("#layer").innerHTML += usedLayers.map((l) => `<option value="${l}">${l}</option>`).join("");

let phase = "";
function renderPhases() {
  $("#phases").innerHTML = D.milestones.map((m) => {
    const n = D.issues.filter((i) => i.milestone === m.key).length;
    return `<button class="phase" aria-pressed="${phase === m.key}" data-k="${m.key}"><span class="k">${m.key}</span><span class="t">${esc(m.title.replace(/^Phase \d: /, ""))}</span><span class="n">${n} issues</span></button>`;
  }).join("");
  const m = D.milestones.find((x) => x.key === phase);
  $("#gatebox").hidden = !m;
  if (m) $("#gatebox").innerHTML = `<b>${esc(m.title)}.</b> Gate: ${esc(m.gate)}`;
}
$("#phases").addEventListener("click", (e) => {
  const b = e.target.closest(".phase"); if (!b) return;
  phase = phase === b.dataset.k ? "" : b.dataset.k; renderPhases(); renderList();
});

function issueHTML(i) {
  const t = typeOf(i);
  const deps = (i.depends || []).map((d) => `<a href="#${d}" data-jump="${d}" title="${esc(byKey[d] ? byKey[d].title : "")}">${d}</a>`).join("");
  const users = D.issues.filter((o) => (o.depends || []).includes(i.key)).map((o) => `<a href="#${o.key}" data-jump="${o.key}" title="${esc(o.title)}">${o.key}</a>`).join("");
  return `<details class="issue" id="${i.key}" data-type="${t}">
    <summary><span class="key">${i.key}</span><span class="title">${esc(i.title)}</span><span class="size">${i.size}</span>
      <span class="chips">${i.labels.map((l) => `<span class="chip ${l.startsWith("type:") ? "type" : ""}">${esc(l)}</span>`).join("")}</span></summary>
    <div class="body">
      <div><h4>Requirements</h4><ul>${i.reqs.map((r) => `<li><span class="reqid">${r}</span> ${esc(reqText[r] || "")}</li>`).join("")}</ul></div>
      <div><h4>Deliverable</h4><ul>${i.deliverable.map((x) => `<li>${code(x)}</li>`).join("")}</ul></div>
      <div><h4>Acceptance criteria</h4><ul>${i.acceptance.map((x) => `<li>${code(x)}</li>`).join("")}</ul></div>
      <div><h4>Verification</h4><ul>${i.verify.map((x) => `<li>${code(x)}</li>`).join("")}</ul></div>
      ${deps ? `<div><h4>Depends on</h4><div class="deps">${deps}</div></div>` : ""}
      ${users ? `<div><h4>Needed by</h4><div class="deps">${users}</div></div>` : ""}
    </div></details>`;
}
function renderList() {
  const q = $("#q").value.trim().toLowerCase(), t = $("#type").value, l = $("#layer").value;
  const shown = D.issues.filter((i) =>
    (!phase || i.milestone === phase) && (!t || typeOf(i) === t) && (!l || layerOf(i) === l) &&
    (!q || JSON.stringify(i).toLowerCase().includes(q)));
  $("#count").textContent = `${shown.length} of ${D.issues.length}`;
  $("#list").innerHTML = shown.map(issueHTML).join("");
}
["#q", "#type", "#layer"].forEach((s) => $(s).addEventListener("input", renderList));
document.addEventListener("click", (e) => {
  const a = e.target.closest("[data-jump]"); if (!a) return;
  e.preventDefault();
  const k = a.dataset.jump;
  if (!document.getElementById(k)) { phase = ""; $("#q").value = ""; $("#type").value = ""; $("#layer").value = ""; renderPhases(); renderList(); }
  const el = document.getElementById(k); if (el) { el.open = true; el.scrollIntoView({ block: "start", behavior: "smooth" }); }
});

const covBy = {}; D.issues.forEach((i) => i.reqs.forEach((r) => (covBy[r] = covBy[r] || []).push(i.key)));
let sec = "", rows = "";
D.reqs.forEach((r) => {
  if (r.section !== sec) { sec = r.section; rows += `<tr><td class="sec" colspan="3">${esc(sec)}</td></tr>`; }
  rows += `<tr><td class="reqid">${r.id}${r.kind === "implied" ? ` <span class="implied">implied</span>` : ""}</td><td>${esc(r.text)}</td>
    <td><div class="deps">${(covBy[r.id] || []).map((k) => `<a href="#${k}" data-jump="${k}">${k}</a>`).join("") || `<span class="errors">none</span>`}</div></td></tr>`;
});
$("#cov").innerHTML = rows;
$("#dod").innerHTML = D.dod.map((d) => `<li>${esc(d)}</li>`).join("");

const tabs = { "tab-issues": "pane-issues", "tab-cov": "pane-cov", "tab-dod": "pane-dod" };
document.querySelector(".tabs").addEventListener("click", (e) => {
  const b = e.target.closest("[role=tab]"); if (!b) return;
  for (const [t, p] of Object.entries(tabs)) {
    const on = t === b.id; document.getElementById(t).setAttribute("aria-selected", on);
    document.getElementById(p).hidden = !on;
  }
  // jump links from coverage go to the issues tab
});
$("#cov").addEventListener("click", (e) => { if (e.target.closest("[data-jump]")) document.getElementById("tab-issues").click(); }, true);
renderPhases(); renderList();
</script>
"""

if __name__ == "__main__":
    sys.exit(main())
