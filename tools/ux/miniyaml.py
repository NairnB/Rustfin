"""A small YAML-subset loader for tasks.yaml (stdlib only; PyYAML is not in the build container).

Supported: a top-level list of mappings ("- key: value", continuation keys indented two spaces), scalars (plain,
"quoted", 'quoted', true/false/null, integers), block lists of scalars under a key ("key:" then "    - item"),
flow maps {a: 1, b: x} and flow lists [a, b] (nestable), and whole-line "# comments". Nothing else.
"""


class YamlError(ValueError):
    pass


def _scalar(s):
    s = s.strip()
    if len(s) >= 2 and s[0] == s[-1] and s[0] in "\"'":
        body = s[1:-1]
        return body.replace('\\"', '"').replace("\\\\", "\\") if s[0] == '"' else body.replace("''", "'")
    if s in ("true", "false"):
        return s == "true"
    if s in ("null", "~", ""):
        return None
    try:
        return int(s)
    except ValueError:
        return s


def _split_top(s, sep=","):
    parts, depth, quote, cur = [], 0, None, ""
    for ch in s:
        if quote:
            cur += ch
            if ch == quote:
                quote = None
        elif ch in "\"'":
            quote = ch
            cur += ch
        elif ch in "{[":
            depth += 1
            cur += ch
        elif ch in "}]":
            depth -= 1
            cur += ch
        elif ch == sep and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        parts.append(cur)
    return parts


def _split_key(s):
    """'key: value' -> (key, value) at the first colon outside quotes."""
    quote = None
    for i, ch in enumerate(s):
        if quote:
            if ch == quote:
                quote = None
        elif ch in "\"'":
            quote = ch
        elif ch == ":" and (i + 1 == len(s) or s[i + 1] == " "):
            return s[:i].strip(), s[i + 1:].strip()
    raise YamlError(f"expected 'key: value': {s!r}")


def _value(s):
    s = s.strip()
    if s.startswith("{") and s.endswith("}"):
        out = {}
        for part in _split_top(s[1:-1]):
            k, v = _split_key(part.strip())
            out[k] = _value(v)
        return out
    if s.startswith("[") and s.endswith("]"):
        return [_value(p) for p in _split_top(s[1:-1])]
    return _scalar(s)


def loads(text):
    items, cur, list_key = [], None, None
    for n, raw in enumerate(text.splitlines(), 1):
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        indent = len(raw) - len(raw.lstrip())
        line = raw.strip()
        try:
            if indent == 0:
                if not line.startswith("- "):
                    raise YamlError("top level must be a list of '- ' items")
                cur, list_key = {}, None
                items.append(cur)
                k, v = _split_key(line[2:])
                cur[k] = _value(v)
            elif line.startswith("- "):
                if cur is None or list_key is None:
                    raise YamlError("list item without a key")
                cur[list_key].append(_scalar(line[2:]) if not line[2:].startswith(("{", "[")) else _value(line[2:]))
            else:
                if cur is None:
                    raise YamlError("key before first item")
                k, v = _split_key(line)
                if v == "":
                    cur[k], list_key = [], k
                else:
                    cur[k], list_key = _value(v), None
        except YamlError as e:
            raise YamlError(f"line {n}: {e}") from None
    return items
