#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Refuse a pull request body without the self-review work-issue's "Land it" asks for: a
# "## Code review" heading whose section names the effort level the code-review skill ran at
# (low, medium, high, xhigh or max), so a PR cannot land with the review skipped.
# Usage: check-code-review.sh < body
# Exits 0 when the section is there; 1, saying what is missing, when it is not.
set -euo pipefail

perl -e '
  use strict;
  use warnings;
  local $/;
  my $body = <STDIN> // "";
  $body =~ s/\r\n?/\n/g;
  # The section: from its heading to the next heading of level 1 or 2, or the end.
  if ($body !~ /^\#\#[ \t]+Code review[ \t]*\n(.*?)(?=^\#{1,2}[ \t]|\z)/msi) {
    print "The body has no \"## Code review\" section: run /code-review high <pr> and record it (work-issue, Land it).\n";
    exit 1;
  }
  my $section = $1;
  if ($section !~ /\b(?:low|medium|high|xhigh|max)\b/i) {
    print "The \"## Code review\" section does not name the effort level the review ran at.\n";
    exit 1;
  }
  exit 0;
'
