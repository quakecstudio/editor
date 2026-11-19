#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Refuse a pull request body without the self-review work-issue's "Land it" asks for: a
# "## Code review" heading whose section names the level the code-review skill ran at, as
# "/code-review high" or "effort: high" (low, medium, high, xhigh or max), so a PR cannot land
# with the review skipped. It also refuses a section that leaves a finding unfixed: an item or
# paragraph that says a finding was not changed, not fixed, left as it is, left for later,
# deferred, skipped, out of scope or kept for a change of its own, unless the item says Justin
# decided it, in those words and with nothing before them in their clause that hedges or negates
# it ("Justin has not decided", "until Justin decides"). Text in code marks is not read. A finding is fixed on the branch, in its proper place, or it is his call
# (development-discipline.md, rule 6). The words are matched as they are written, so an item
# about a fixed finding that happens to use one ("no longer deferred") is reworded.
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
    \b(?:not|never)\s+(?:changed|fixed|addressed|done)\b
    | \bwon(?:\x27|\xE2\x80\x99)?t\s+fix\b
    | \b(?:left|kept|stays?)\s+as\s+(?:it\s+is|they\s+are|is)\b
    | \bleft\s+(?:for\s+later|alone|out)\b
    | \bdefer\w*\b | \bskipped\b | \bout\s+of\s+scope\b
    | \b(?:its|their|a)\s+(?:own|later|separate)\s+(?:change|PR|pull\s+request)\b
  }xi;
  # Whether an item says Justin decided it: "Justin decided" or "Justin has decided", or
  # "decided by Justin", in a sentence that does not hedge or negate it before those words
  # ("Justin has not decided", "until Justin decides", "if Justin decided", "Justin may decide").
  sub decided {
    my ($item) = @_;
    my $said = qr{\bJustin(?:\s+has)?\s+decided\b | \bdecided\s+by\s+Justin\b}xi;
    my $hedge = qr{
      \b(?:not|never|yet|until|unless|if|whether|may|might|will|would|should|could|can
         |pending|awaiting|waiting|\w+n(?:\x27|\xE2\x80\x99)t)\b
    }xi;
    for my $sentence (split /(?<=[.!?;])\s+/, $item) {
      next unless $sentence =~ $said;
      # The clause the words are in: what follows the last ":", ";" or "," before them, so
      # "not changed: Justin decided it stays" is his decision and "not fixed, pending what
      # Justin decided to look at" is none.
      my $before = substr($sentence, 0, $-[0]);
      $before =~ s/^.*[:;,]//s;
      return 1 unless $before =~ $hedge;
    }
    return 0;
  }
  my $bad = 0;
  for my $item (split /\n[ \t]*\n|\n(?=[ \t]*(?:[-*+]|\d+[.)])[ \t])/, $section) {
    # A name in code marks, such as `deferred_store`, is not prose.
    my $prose = $item =~ s/`[^`\n]*`//gr;
    next unless $prose =~ $deferred;
    next if decided($item);
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
