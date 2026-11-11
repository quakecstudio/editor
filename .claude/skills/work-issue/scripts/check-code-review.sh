#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Refuse a pull request body without the self-review work-issue's "Land it" asks for: a
# "## Code review" heading whose section names the level the code-review skill ran at, as
# "/code-review high" or "effort: high" (low, medium, high, xhigh or max), so a PR cannot land
# with the review skipped.
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
  exit 0;
'
