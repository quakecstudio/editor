; SPDX-License-Identifier: GPL-3.0-or-later
; Copyright (C) 2026 QuakeC Studio contributors
;
; Capture names are the tree-sitter CLI's standard set. A later pattern takes precedence over
; an earlier one for the same node, so the general patterns come first.

(identifier) @variable

((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]*$"))

((identifier) @variable.builtin
  (#any-of? @variable.builtin "self" "other" "world" "this" "super"))

; Keywords and types

[
  "local"
  "return"
  "if"
  "else"
  "while"
  "do"
  ; FTEQCC
  "for"
  "switch"
  "case"
  "default"
  "break"
  "continue"
  "goto"
  "until"
  "not"
  "sizeof"
  "struct"
  "union"
  "enum"
  "enumflags"
  "class"
  "accessor"
  "get"
  "set"
  "inline"
] @keyword

[
  (storage_qualifier)
  (parameter_qualifier)
  (deprecated_attribute)
  (access_specifier)
] @keyword

[
  (simple_type)
  (sized_type)
] @type.builtin

(type_identifier) @type

(struct_specifier
  name: (identifier) @type)

(struct_specifier
  parent: (identifier) @type)

(class_specifier
  name: (identifier) @type)

(class_specifier
  parent: (identifier) @type)

(enum_specifier
  name: (identifier) @type)

(accessor_specifier
  name: (identifier) @type)

(scoped_identifier
  scope: (identifier) @type)

; Operators and punctuation

[
  "="
  "+="
  "-="
  "*="
  "/="
  "%="
  "|="
  "&="
  "&~="
  "^="
  "<<="
  ">>="
  "(+)"
  "(-)"
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
  "<=>"
  "+"
  "-"
  "*"
  "/"
  "%"
  "&"
  "|"
  "^"
  "<<"
  ">>"
  "><"
  "&&"
  "||"
  "!"
  "~"
  "++"
  "--"
  "?"
  "->"
] @operator

(conditional_expression
  ":" @operator)

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ";"
  "."
  "::"
  ":"
] @punctuation.delimiter

[
  "..."
  ".."
] @punctuation.special

(builtin
  "#" @punctuation.special)

; Literals

(float_literal) @number

(vector_literal
  "'" @number)

[
  (string_literal)
  (char_literal)
] @string

(escape_sequence) @string.special

; `$` commands and frame macros

(frame_definition
  [
    "$"
    "frame"
    "framesave"
  ] @keyword)

(model_command
  "$" @keyword
  name: (identifier) @keyword)

(command_arguments) @string.special

(frame_name) @constant

(frame_reference) @constant

; Preprocessor

[
  "#include"
  "#define"
  "#undef"
  "#ifdef"
  "#ifndef"
  "#else"
  "#endif"
  (preproc_directive)
] @keyword

(preproc_define
  name: (identifier) @constant)

(preproc_params
  (identifier) @variable.parameter)

(macro_invocation
  name: (identifier) @constant)

(macro_invocation
  name: (identifier) @function
  arguments: (argument_list))

; Uses

(field_expression
  field: (identifier) @property)

(designated_initializer
  member: (identifier) @property)

(call_expression
  function: (identifier) @function)

(call_expression
  function: (scoped_identifier
    name: (identifier) @function))

(call_expression
  function: (field_expression
    field: (identifier) @function))

; Definitions

(parameter
  name: (identifier) @variable.parameter)

(state
  next: (identifier) @function)

(enumerator
  name: (identifier) @constant)

(accessor_member
  name: (identifier) @property)

(declaration
  type: (field_type)
  declarator: (declarator
    name: (identifier) @property))

(declaration
  type: (function_type)
  declarator: (declarator
    name: (identifier) @function))

(declarator
  name: (identifier) @function
  parameters: (parameter_list))

(declarator
  name: (scoped_identifier
    name: (identifier) @function)
  parameters: (parameter_list))

(declaration
  type: (function_type)
  declarator: (declarator
    name: (identifier) @function.builtin
    value: (builtin)))

(comment) @comment
