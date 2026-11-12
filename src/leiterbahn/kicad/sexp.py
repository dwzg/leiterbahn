"""Minimal S-expression reader/writer for KiCad files."""
import re


class Sym(str):
    """Bare (unquoted) atom."""
    pass

_tok = re.compile(r'\s*(?:(\()|(\))|"((?:[^"\\]|\\.)*)"|([^\s()"]+))', re.S)

def parse(text):
    stack = [[]]
    pos = 0
    n = len(text)
    while pos < n:
        m = _tok.match(text, pos)
        if not m:
            if text[pos:].strip() == "":
                break
            raise ValueError("parse error at %d: %r" % (pos, text[pos:pos + 40]))
        pos = m.end()
        lp, rp, q, a = m.groups()
        if lp:
            stack.append([])
        elif rp:
            node = stack.pop()
            stack[-1].append(node)
        elif q is not None:
            stack[-1].append(q.replace('\\"', '"').replace("\\\\", "\\"))
        else:
            stack[-1].append(Sym(a))
    return stack[0][0] if len(stack[0]) == 1 else stack[0]

def _atom(x):
    if isinstance(x, Sym):
        return str(x)
    if isinstance(x, bool):
        return "yes" if x else "no"
    if isinstance(x, (int, float)):
        s = ("%.4f" % x).rstrip("0").rstrip(".") if isinstance(x, float) else str(x)
        return "0" if s in ("-0", "") else s
    s = str(x).replace("\\", "\\\\").replace('"', '\\"')
    return '"%s"' % s

def dump(node, indent=0):
    if not isinstance(node, list):
        return _atom(node)
    if node and isinstance(node[0], str) and not isinstance(node[0], Sym):
        node = [Sym(node[0])] + node[1:]   # list heads are always bare keywords
    simple = all(not isinstance(c, list) for c in node)
    if simple:
        return "(" + " ".join(_atom(c) for c in node) + ")"
    pad = "\t" * (indent + 1)
    head = []
    for c in node:
        if isinstance(c, list):
            break
        head.append(_atom(c))
    out = "(" + " ".join(head)
    for c in node[len(head):]:
        out += "\n" + pad + dump(c, indent + 1)
    return out + "\n" + "\t" * indent + ")"

def find(node, key):
    """All direct children lists whose head is key."""
    return [c for c in node if isinstance(c, list) and c and c[0] == key]

def find1(node, key, default=None):
    r = find(node, key)
    return r[0] if r else default

def walk(node):
    yield node
    for c in node:
        if isinstance(c, list):
            yield from walk(c)
