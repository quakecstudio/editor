# SPDX-License-Identifier: GPL-3.0-or-later
"""The binary form of fbx (MDL-11), written: a tree of records, each a name, its properties
and the records nested in it, and the same tree as the text form."""
import struct, zlib
def prop(v):
    if isinstance(v, bool):
        return b"C" + struct.pack("<B", v)
    if isinstance(v, int):
        return b"L" + struct.pack("<q", v) if abs(v) >= 2 ** 31 else b"I" + struct.pack("<i", v)
    if isinstance(v, float):
        return b"D" + struct.pack("<d", v)
    if isinstance(v, str):
        raw = v.encode()
        return b"S" + struct.pack("<I", len(raw)) + raw
    kind, vals = v                                 # an array: its kind and its values
    # The file's "l" is a 64-bit integer, which struct calls "q".
    raw = struct.pack("<%d%s" % (len(vals), "q" if kind == "l" else kind), *vals)
    packed = zlib.compress(raw)
    return kind.encode() + struct.pack("<III", len(vals), 1, len(packed)) + packed
def record(name, props, children, wide, at):
    """A record that starts at the file's byte `at`."""
    head = "<QQQB" if wide else "<IIIB"
    body = b"".join(prop(v) for v in props)
    start = at + struct.calcsize(head) + len(name)
    nested = b""
    for child in children:
        nested += record(*child, wide, start + len(body) + len(nested))
    if children:
        nested += bytes(struct.calcsize(head))
    end = start + len(body) + len(nested)
    return struct.pack(head, end, len(props), len(body), len(name)) + name.encode() + body + nested
def named(name, cls):
    return name + "\x00\x01" + cls
def p70(*lines):
    return ("Properties70", [], [("P", list(line), []) for line in lines])
def hexed(data):
    return "\n".join(data[i:i + 32].hex() for i in range(0, len(data), 32)) + "\n"
def text(node, depth=0):
    """The record `node`, a name, its properties and its nested records, as the text form
    writes one."""
    name, props, children = node
    tabs = "\t" * depth
    def word(v):
        if isinstance(v, bool):
            return "Y" if v else "N"
        if isinstance(v, str):
            # A binary file's `Name\0\1Class` is the text form's `Class::Name`.
            own, _, cls = v.partition("\x00\x01")
            return '"' + (cls + "::" + own if cls else own) + '"'
        if isinstance(v, tuple):
            return "*%d {\n%s\ta: %s\n%s}" % (len(v[1]), tabs, ",".join(repr(x) for x in v[1]), tabs)
        return repr(v)
    line = tabs + name + ": " + ", ".join(word(v) for v in props)
    if not children:
        return line + "\n"
    return line + " {\n" + "".join(text(child, depth + 1) for child in children) + tabs + "}\n"
