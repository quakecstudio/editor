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
# `CLOSES #10`, or `CLOSES: #10`." An issue URL after a keyword is matched too.
#
# The rule is deliberately blunt: a sentence holding both a keyword reference and a negation or
# hedge word (not, no, never, nor, without, cannot, any "n't" word, partly, partially) is
# refused, wherever the two are in it. A sentence ends at ".", "!", "?" or ";" followed by
# white space, at a blank line, before a line that starts a Markdown list item, heading, table
# row, quote or fence, and after a heading or table row; a single line break inside a paragraph
# does not end one.
set -euo pipefail

perl -e '
  use strict;
  use warnings;
  local $/;
  my $body = <STDIN> // "";
  $body =~ s/\r\n?/\n/g;
  $body =~ s/^((?:#{1,6}[ \t]|\|)[^\n]*)\n/$1\n\n/mg;
  my $keyword = qr{
    \b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\b :? \s*
    ( (?:[\w.-]+/[\w.-]+)?\#\d+ | https?://\S+/(?:issues|pull)/\d+ )
  }xi;
  my $negation = qr{\b(?:not|no|never|nor|without|cannot|\w+n(?:\x27|\xE2\x80\x99)t|partly|partially)\b}i;
  my @sentences = split m{
    (?<=[.!?;])\s+
    | \n[ \t]*\n
    | \n(?=[ \t]*(?:[-*+][ \t]|\d+[.)][ \t]|\#|\||>|```|~~~))
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
    print "  GitHub closes the issue on merge whatever the sentence says. Write \"Refs $_\" for an issue this PR must leave open.\n" for @refs;
  }
  exit $bad;
'
