; SPDX-License-Identifier: GPL-3.0-or-later
; Copyright (C) 2026 QuakeC Studio contributors

; TODO, FIXME and similar markers in comments, for editors with a comment grammar.
((comment) @injection.content
  (#set! injection.language "comment"))

; QC-9.3: a macro body is QuakeC text.
((preproc_arg) @injection.content
  (#set! injection.language "quakec"))
