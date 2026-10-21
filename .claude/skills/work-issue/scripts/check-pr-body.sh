#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Refuse a pull request body that names an issue after a closing keyword in a negated or hedged
# sentence, such as "this does not close #84": GitHub reads the keyword and the reference and
# closes the issue on merge, whatever the words around them say.
# Usage: check-pr-body.sh < body
# Exits 0 when the body is safe to merge; 1, printing each offending sentence, when it is not.
#
# The keywords, from GitHub's "Linking a pull request to an issue"
# (https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue,
# fetched 2026-10-20), "Linking a pull request to an issue using a keyword":
#   close, closes, closed, fix, fixes, fixed, resolve, resolves, resolved
# with the syntax "KEYWORD #ISSUE-NUMBER" and "KEYWORD OWNER/REPOSITORY#ISSUE-NUMBER", and
# "The keywords can be followed by colons or in uppercase. For example: `Closes: #10`,
# `CLOSES #10`, or `CLOSES: #10`." An issue URL after a keyword, and emphasis or code marks
# between the keyword and the reference (`**Does not close** #84`), are matched too.
#
# The rule is deliberately blunt: a sentence holding both a keyword reference and a negation or
# hedge word is refused, wherever the two are in it. The words: not, no, never, nor, without,
# cannot, any "n't" word and the common ones written without the apostrophe ("doesnt"), partly,
# partially, "in part", and the deferring later, follow-up, future, will, once, until,
# eventually. A sentence ends at ".", "!", "?" or ";" followed by white space, at a blank line,
# before a line that starts a Markdown list item, heading, table row, quote or fence (unless that
# line goes on with an issue reference, which GitHub may read with the line before), and after a
# heading or table row; a single line break inside a paragraph does not end one.
#
# A false refusal costs a reworded body; a missed one closes an issue, so the rule errs towards
# refusing. GitHub's own list (`gh pr view --json closingIssuesReferences`) says which issues a
# body closes but not which it meant to, so it cannot replace the check.
set -euo pipefail

perl -e '
  use strict;
  use warnings;
  local $/;
  my $body = <STDIN> // "";
  $body =~ s/\r\n?/\n/g;
  $body =~ s/^((?:#{1,6}[ \t]|\|)[^\n]*)\n/$1\n\n/mg;
  my $ref = qr{(?:[\w.-]+/[\w.-]+)?\#\d+ | https?://\S+/(?:issues|pull)/\d+}x;
  my $keyword = qr{
    \b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\b [*_`]* :? [*_`]* \s* (?:[-+>|]\s*)? [*_`]* ($ref)
  }xi;
  my $negation = qr{
    \b(?:not|no|never|nor|without|cannot|\w+n(?:\x27|\xE2\x80\x99)t
       |(?:does|do|did|wo|is|are|was|ca|should|would|could)nt
       |partly|partially|in\s+part|later|follow-?up|future|will|once|until|eventually)\b
  }xi;
  my @sentences = split m{
    (?<=[.!?;])\s+
    | \n[ \t]*\n
    | \n(?=[ \t]*(?:[-*+][ \t]|\d+[.)][ \t]|\#{1,6}[ \t]|\||>|```|~~~))
        (?![ \t]*(?:[-*+>|][ \t]*)?$ref)
  }x, $body;
  my $bad = 0;
  for my $sentence (@sentences) {
    next unless defined $sentence;
    my $flat = $sentence =~ s/\s+/ /gr;
    $flat =~ s/^ | $//g;
    my @refs;
    while ($flat =~ /$keyword/g) { push @refs, $1; }
    next unless @refs && $flat =~ $negation;
    $bad = 1;
    print "Refused: a closing keyword next to an issue in a negated or hedged sentence:\n";
    print "  $flat\n";
    print "  GitHub closes the issue on merge whatever the sentence says. Write \"Refs $_\" for an issue this PR must leave open, or put \"Closes $_\" in a sentence of its own if it should close.\n" for @refs;
  }
  exit $bad;
'
