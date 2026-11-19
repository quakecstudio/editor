#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Refuse a pull request body without the self-review work-issue's "Land it" asks for: a
# "## Code review" heading whose section names the level the code-review skill ran at, as
# "/code-review high" or "effort: high" (low, medium, high, xhigh or max), so a PR cannot land
# with the review skipped. It also refuses a section that leaves a finding unfixed: an item or
# paragraph that says a finding was not changed, not fixed, left as it is, deferred or kept for
# a change of its own, unless it names Justin as having decided so. A finding is fixed on the
# branch, in its proper place, or it is his call (development-discipline.md, rule 6).
# Usage: check-code-review.sh < body
# Exits 0 when the section is there; 1, saying what is missing, when it is not.
set -euo pipefail

perl -e '
  use strict;
  use warnings;
  local $/;
  my $body = <STDIN> // "";
  $body =~ s/\r\n?/\n/g;
  # The section: from its heading (anything may follow "Code review" on the line) to the next
  # level-2 heading, or the end; a "# " line inside it, such as a comment in a code block,
  # does not end it.
  if ($body !~ /^\#\#[ \t]+Code review[^\n]*\n(.*?)(?=^\#\#[ \t]|\z)/msi) {
    print "The body has no \"## Code review\" section: run /code-review high <pr> and record it (work-issue, Land it).\n";
    exit 1;
  }
  my $section = $1;
  # The level as the skill is run, "/code-review high", or as "effort high" or "effort: high".
  if ($section !~ m{(?:/code-review|\beffort:?)[ \t]+(?:low|medium|high|xhigh|max)\b}i) {
    print "The \"## Code review\" section does not name the level the review ran at, as \"/code-review high\" or \"effort: high\".\n";
    exit 1;
  }
  # A finding left unfixed: each list item or paragraph is read on its own.
  my $deferred = qr{
    \bnot\s+(?:changed|fixed|addressed|done)\b | \bunchanged\b | \bleft\s+(?:as|for|to|alone)\b
    | \bdeferred?\b | \b(?:its|their)\s+own\s+(?:change|PR|pull\s+request)\b | \bfollow-?up\b
  }xi;
  my $bad = 0;
  for my $item (split /\n[ \t]*\n|\n(?=[ \t]*(?:[-*+]|\d+[.)])[ \t])/, $section) {
    next unless $item =~ $deferred;
    next if $item =~ /\bJustin\b/;
    my $flat = $item =~ s/\s+/ /gr;
    print "The \"## Code review\" section leaves a finding unfixed: $flat\n";
    $bad = 1;
  }
  if ($bad) {
    print "Fix each finding on the branch, in its proper place, or record that Justin decided it stays.\n";
    exit 1;
  }
  exit 0;
'
