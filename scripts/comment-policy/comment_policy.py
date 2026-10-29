import argparse
import configparser
import ast
import hashlib
from functools import lru_cache
from html.parser import HTMLParser
import io
import json
import re
import subprocess
import sys
import tokenize
import tomllib
import xml.etree.ElementTree as ET
import xml.parsers.expat
from dataclasses import dataclass
from pathlib import Path

LANGUAGES = {
    ".c": "c", ".h": "cpp", ".cpp": "cpp", ".cc": "cpp", ".cxx": "cpp", ".hpp": "cpp", ".hxx": "cpp", ".go": "go", ".java": "java", ".kt": "kotlin", ".swift": "swift", ".lua": "lua", ".proto": "proto", ".pyw": "python", ".xml": "xml", ".svg": "xml", ".rs": "rust", ".py": "python", ".pyi": "python", ".mk": "make", ".js": "javascript", ".mjs": "javascript",
    ".cjs": "javascript", ".ts": "typescript", ".tsx": "tsx", ".jsx": "tsx",
    ".css": "css", ".html": "html", ".htm": "html", ".sh": "bash",
    ".bash": "bash", ".bats": "bash", ".toml": "toml", ".yml": "yaml", ".yaml": "yaml",
    ".sql": "sql", ".json": "json", ".jsonc": "jsonc", ".ipynb": "notebook",
}
KNOWN_UNSUPPORTED = {".R", ".r", ".fish", ".ps1", ".vue", ".svelte", ".pine", ".scss", ".less", ".sass", ".rb", ".pl", ".awk", ".sed", ".conf", ".ini", ".cfg", ".ex", ".exs", ".hs", ".scala", ".cs", ".fs", ".fsx", ".m", ".mm", ".jl", ".nix", ".zig"}
COMMENT_TYPES = {"comment", "line_comment", "block_comment", "html_comment"}
LEGAL = re.compile(rb"copyright|SPDX-License|licensed under|permission is hereby granted|redistribution and use", re.I)


class Refusal(ValueError):
    pass


@dataclass(frozen=True)
class Comment:
    start: int
    end: int
    text: bytes
    language: str
    doc: bool = False
    inner: bool = False
    doc_text: bytes = b""


def language_for(path, data):
    if path.suffix in {".yml", ".yaml"} and ".github" in path.parts and "workflows" in path.parts:
        return "workflow"
    if path.name in {"Makefile", "GNUmakefile", "makefile"} or path.name.startswith("Makefile."):
        return "make"
    if path.name == ".stignore":
        return "syncthing"
    if path.name == "Doxyfile":
        return "doxygen"
    if path.name in {".gitignore", ".dockerignore"} or path.name.endswith(".dockerignore"):
        return "ignore"
    if path.name == ".gitattributes":
        return "attributes"
    if path.suffix in {".service", ".timer"}:
        return "systemd"
    if path.name.startswith(".env") or path.suffix == ".env" or path.name.endswith(".env.example"):
        return "bash"
    if path.name == "config" and path.parent.name == ".cargo":
        return "toml"
    if path.name.endswith(".toml.in"):
        return "toml"
    if path.name.endswith(".js.tpl"):
        return "javascript"
    if path.name == "Dockerfile" or path.name.startswith("Dockerfile.") or path.suffix == ".dockerfile":
        return "dockerfile"
    if path.name in {"Cargo.lock", "uv.lock"} or path.name.endswith(".toml.orig"):
        return "toml"
    if path.suffix in LANGUAGES:
        return LANGUAGES[path.suffix]
    if data.startswith(b"#!"):
        first = data.splitlines()[0]
        if b"python" in first:
            return "python"
        if re.search(rb"(?:/|\s)(?:ba|da|z|k)?sh(?:\s|$)", first):
            return "bash"
        if b"node" in first:
            return "javascript"
    raise Refusal(f"unsupported source language: {path}")


def walk(node):
    yield node
    for child in node.children:
        yield from walk(child)


@lru_cache
def parser_for(language):
    from tree_sitter_language_pack import get_parser
    return get_parser("json" if language == "jsonc" else language)


def parse_tree(data, language):
    if language == "json":
        try:
            json.loads(data)
        except ValueError as error:
            raise Refusal(f"JSON parse failure: {error}") from error
    tree = parser_for(language).parse(data)
    if tree.root_node.has_error:
        errors = [f"{node.type}@{node.start_point.row + 1}:{node.start_point.column + 1}" for node in walk(tree.root_node) if node.is_error or node.is_missing or (node.has_error and not any(child.has_error for child in node.children))]
        raise Refusal(f"{language} parse failure: {', '.join(errors[:8])}")
    return tree


def python_comments(data):
    text = data.decode("utf-8")
    lines = text.splitlines(keepends=True)
    offsets = [0]
    for line in lines:
        offsets.append(offsets[-1] + len(line.encode("utf-8")))
    result = []
    for token in tokenize.tokenize(io.BytesIO(data).readline):
        if token.type == tokenize.COMMENT:
            row, col = token.start
            endrow, endcol = token.end
            start = offsets[row - 1] + len(lines[row - 1][:col].encode("utf-8"))
            end = offsets[endrow - 1] + len(lines[endrow - 1][:endcol].encode("utf-8"))
            result.append(Comment(start, end, data[start:end], "python"))
    ast.parse(data)
    return result


def tree_comments(data, language, offset=0):
    tree = parse_tree(data, language)
    result = []
    for node in walk(tree.root_node):
        if node.type in COMMENT_TYPES and not any(parent.type in COMMENT_TYPES for parent in ancestors(node)):
            doc = node.child_by_field_name("doc") if language == "rust" else None
            inner = any(child.type == "inner_doc_comment_marker" for child in node.children)
            body = data[doc.start_byte:doc.end_byte] if doc else b""
            if doc and node.type == "line_comment":
                body = body.removesuffix(b"\n").removesuffix(b"\r")
            result.append(Comment(offset + node.start_byte, offset + node.end_byte, data[node.start_byte:node.end_byte], language, doc is not None, inner, body))
    return result


def ancestors(node):
    parent = node.parent
    while parent is not None:
        yield parent
        parent = parent.parent


def sql_comment_free(data, language):
    return language == "sql" and not any(marker in data for marker in (b"--", b"/*", b"#"))


def line_comments(data, language):
    if language == "systemd":
        parser = configparser.ConfigParser(interpolation=None, strict=False, empty_lines_in_values=False)
        parser.optionxform = str
        try:
            parser.read_string(data.decode("utf-8"))
        except configparser.Error as error:
            raise Refusal(f"systemd parse failure: {error}") from error
    found = []
    offset = 0
    for line in data.splitlines(keepends=True):
        content = line.rstrip(b"\r\n")
        candidate = content if language == "ignore" else content.lstrip(b" \t")
        if (candidate.startswith(b"//") if language == "syncthing" else candidate.startswith(b"#")) or language == "systemd" and candidate.startswith(b";"):
            start = offset + len(content) - len(candidate)
            found.append(Comment(start, offset + len(content), candidate, language))
        offset += len(line)
    return found


def sql_lex(data):
    found = []
    tokens = []
    index = 0
    while index < len(data):
        start = index
        if data[index:index + 1].isspace():
            index += 1
            continue
        if data.startswith(b"--", index):
            ending = re.search(rb"[\r\n]", data[index:])
            index = len(data) if ending is None else index + ending.start()
            found.append(Comment(start, index, data[start:index], "sql"))
            continue
        if data.startswith(b"/*", index):
            depth = 1
            index += 2
            while index < len(data) and depth:
                if data.startswith(b"/*", index):
                    depth += 1
                    index += 2
                elif data.startswith(b"*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            if depth:
                raise Refusal("unterminated SQL block comment")
            found.append(Comment(start, index, data[start:index], "sql"))
            continue
        quote = data[index:index + 1]
        if quote == b"#":
            raise Refusal("SQL hash comments/operators are outside the supported dialect; use -- comments")
        if quote == b"[":
            end = data.find(b"]", index + 1)
            if end < 0 or any(marker in data[index + 1:end] for marker in (b"--", b"/*", b"*/")):
                raise Refusal("ambiguous SQL bracket syntax with comment delimiter; use double-quoted identifiers")
        if quote in {b"'", b'"', b"`"}:
            index += 1
            escaped = quote == b"'" and start > 0 and data[start - 1:start].lower() == b"e" and (start < 2 or not chr(data[start - 2]).isalnum())
            while index < len(data):
                if escaped and data[index:index + 1] == b"\\":
                    index += 2
                elif data[index:index + 1] == quote:
                    index += 1
                    if data[index:index + 1] == quote:
                        index += 1
                    else:
                        break
                else:
                    index += 1
            else:
                raise Refusal("unterminated SQL quoted literal")
            tokens.append(data[start:index])
            continue
        dollar = re.match(rb"\$(?:[A-Za-z_][A-Za-z_0-9]*)?\$", data[index:])
        if dollar and (start == 0 or data[start - 1:start] not in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_$"):
            marker = dollar.group()
            end = data.find(marker, index + len(marker))
            if end < 0:
                raise Refusal("unterminated SQL dollar literal")
            index = end + len(marker)
            tokens.append(data[start:index])
            continue
        word = re.match(rb"[A-Za-z_0-9]+", data[index:])
        if word:
            index += len(word.group())
        else:
            index += 1
        tokens.append(data[start:index])
    return found, tuple(tokens)


def make_comments(data):
    if re.search(rb"(?m)^[ \t]*(?:(?:override|export)\s+)?define\b|^[ \t]*\.RECIPEPREFIX\s*[:?+]?=", data):
        raise Refusal("Make define bodies and custom recipe prefixes are unsupported")
    found = []
    lines = data.splitlines(keepends=True)
    offsets = []
    offset = 0
    for line in lines:
        offsets.append(offset)
        offset += len(line)
    row = 0
    while row < len(lines):
        line = lines[row]
        if line.startswith(b"\t"):
            group = []
            mapping = []
            while row < len(lines) and (lines[row].startswith(b"\t") or not lines[row].strip()):
                item = lines[row]
                removed = 1 if item.startswith(b"\t") else 0
                if removed:
                    while removed < len(item) and item[removed:removed + 1] in {b"@", b"-", b"+"}:
                        removed += 1
                body = item[removed:]
                group.append(body)
                mapping.extend(range(offsets[row] + removed, offsets[row] + len(item)))
                row += 1
            script = b"".join(group)
            if b"#" in script:
                for comment in tree_comments(script, "bash"):
                    start = mapping[comment.start]
                    end = mapping[comment.end - 1] + 1
                    original = data[start:end]
                    if original != comment.text:
                        raise Refusal("Make recipe comment crosses stripped recipe prefix")
                    found.append(Comment(start, end, original, "bash"))
            continue
        start = offsets[row]
        row += 1
        while line.rstrip(b"\r\n").endswith(b"\\") and row < len(lines):
            line += lines[row]
            row += 1
        index = 0
        stack = []
        while index < len(line):
            if line[index:index + 1] == b"\\":
                index += 2
                continue
            if line[index:index + 2] in {b"$(", b"${"}:
                close = b")" if line[index + 1:index + 2] == b"(" else b"}"
                stack.append((close, index + 2, line[index + 2:].startswith(b"shell ")))
                index += 2
                continue
            if stack and line[index:index + 1] == (b"(" if stack[-1][0] == b")" else b"{"):
                stack.append((stack[-1][0], index + 1, False))
                index += 1
                continue
            if stack and line[index:index + 1] == stack[-1][0]:
                close, body_start, shell = stack.pop()
                if shell and b"#" in line[body_start + 6:index]:
                    body = line[body_start + 6:index]
                    for comment in tree_comments(body, "bash"):
                        found.append(Comment(start + body_start + 6 + comment.start, start + body_start + 6 + comment.end, comment.text, "bash"))
                index += 1
                continue
            if line[index:index + 1] == b"#" and not stack:
                end = len(line.rstrip(b"\r\n"))
                found.append(Comment(start + index, start + end, line[index:end], "make"))
                break
            index += 1
        if stack:
            raise Refusal("unclosed Make variable expansion")
    normalized = []
    for comment in found:
        line_start = data.rfind(b"\n", 0, comment.start) + 1
        start = line_start if not data[line_start:comment.start].strip() else comment.start
        normalized.append(Comment(start, comment.end, data[start:comment.end], comment.language))
    return normalized


def xml_comments(data):
    parser = xml.parsers.expat.ParserCreate()
    found = []
    def found_comment(value):
        start = parser.CurrentByteIndex
        end = data.find(b"-->", start)
        if end < 0:
            raise Refusal("unterminated XML comment")
        found.append(Comment(start, end + 3, data[start:end + 3], "xml"))
    parser.CommentHandler = found_comment
    try:
        parser.Parse(data, True)
    except xml.parsers.expat.ExpatError as error:
        raise Refusal(f"XML parse failure: {error}") from error
    return found


def doxygen_comments(data):
    found = []
    index = 0
    quoted = False
    while index < len(data):
        value = data[index:index + 1]
        if value == b"\\":
            index += 2
            continue
        if value == b'"':
            quoted = not quoted
        elif value == b"#" and not quoted:
            end = re.search(rb"[\r\n]", data[index:])
            stop = len(data) if end is None else index + end.start()
            found.append(Comment(index, stop, data[index:stop], "doxygen"))
            index = stop
            continue
        index += 1
    if quoted:
        raise Refusal("unterminated Doxygen quoted value")
    return found


def github_expression_mask(data):
    output = bytearray(data)
    index = 0
    while index < len(data):
        if not data.startswith(b"${{", index):
            index += 1
            continue
        start = index
        index += 3
        quoted = False
        while index < len(data):
            if data[index:index + 1] == b"'":
                if quoted and data[index:index + 2] == b"''":
                    index += 2
                    continue
                quoted = not quoted
            elif not quoted and data.startswith(b"}}", index):
                index += 2
                output[start:index] = b"x" * (index - start)
                break
            index += 1
        else:
            raise Refusal("unterminated GitHub expression")
    return bytes(output)


def workflow_comments(data):
    tree = parse_tree(data, "yaml")
    found = tree_comments(data, "yaml")
    for pair in walk(tree.root_node):
        if pair.type not in {"block_mapping_pair", "flow_pair"}:
            continue
        key = pair.child_by_field_name("key")
        value = pair.child_by_field_name("value")
        if key is None or key.text.strip(b"\"'") != b"run" or value is None:
            continue
        if b"#" not in value.text:
            continue
        siblings = pair.parent.children if pair.parent else []
        shell = None
        for sibling in siblings:
            other_key = sibling.child_by_field_name("key")
            other_value = sibling.child_by_field_name("value")
            if other_key is not None and other_key.text.strip(b"\"'") == b"shell" and other_value is not None:
                shell = other_value.text.strip(b"\"'").split()[0]
        if shell not in {None, b"bash", b"sh"}:
            raise Refusal("workflow run shell with possible comments is unsupported; use bash/sh or a separate source file")
        if shell is None and re.search(rb"(?i)windows|pwsh|powershell|shell:\s*(?!bash|sh)[A-Za-z]", data):
            raise Refusal("workflow run default shell is ambiguous; declare shell: bash or sh")
        scalar = next((node for node in walk(value) if node.type == "block_scalar"), None)
        if scalar is None:
            if any(node.type in {"double_quote_scalar", "single_quote_scalar"} for node in walk(value)):
                raise Refusal("quoted workflow run with possible comments is unsupported; use a literal block")
            source = value.text
            mapping = list(range(value.start_byte, value.end_byte))
        else:
            header, sep, body = scalar.text.partition(b"\n")
            if not header.startswith(b"|"):
                raise Refusal("folded workflow run with possible comments is unsupported; use a literal block")
            if not sep:
                continue
            lines = body.splitlines(keepends=True)
            indentation = min((len(line) - len(line.lstrip(b" ")) for line in lines if line.strip()), default=0)
            fragments = []
            mapping = []
            offset = scalar.start_byte + len(header) + 1
            for line in lines:
                removed = min(indentation, len(line) - len(line.lstrip(b" ")))
                fragments.append(line[removed:])
                mapping.extend(range(offset + removed, offset + len(line)))
                offset += len(line)
            source = b"".join(fragments)
        masked = github_expression_mask(source)
        for comment in tree_comments(masked, "bash"):
            start = mapping[comment.start]
            end = mapping[comment.end - 1] + 1
            if data[start:end] != source[comment.start:comment.end]:
                raise Refusal("workflow shell comment crosses stripped scalar indentation")
            found.append(Comment(start, end, data[start:end], "bash"))
    return found


def comments(data, language):
    data.decode("utf-8")
    if language == "workflow":
        return workflow_comments(data)
    if language == "xml":
        return xml_comments(data)
    if language == "doxygen":
        return doxygen_comments(data)
    if language == "make":
        return make_comments(data)
    if language in {"ignore", "attributes", "systemd", "syncthing"}:
        return line_comments(data, language)
    if language == "sql":
        return sql_lex(data)[0]
    return python_comments(data) if language == "python" else html_analysis(data)[0] if language == "html" else tree_comments(data, language)


def tooling_directive(comment):
    patterns = {
        "python": rb"^#\s*(?:noqa\b|type:|fmt:|ruff:|pylint:|pyright:|mypy:)",
        "bash": rb"^#\s*(?:shellcheck\b|shfmt\b|SBATCH\b|PBS\b)",
        "javascript": rb"(?:@ts-|eslint|prettier|istanbul|c8\s+ignore|sourceMappingURL|sourceURL|[#@]__PURE__|webpack|vite-ignore)",
        "typescript": rb"(?:@ts-|eslint|prettier|istanbul|c8\s+ignore|sourceMappingURL|sourceURL|[#@]__PURE__|webpack|vite-ignore)",
        "tsx": rb"(?:@ts-|eslint|prettier|istanbul|c8\s+ignore|sourceMappingURL|sourceURL|[#@]__PURE__|webpack|vite-ignore)",
        "css": rb"stylelint|prettier|sourceMappingURL",
        "html": rb"^<!--\s*\[if\b|\[endif\]",
        "rust": rb"^//\s*(?:@|cargo-deps:|rustfmt-skip)",
    }
    pattern = patterns.get(comment.language)
    return pattern is not None and re.search(pattern, comment.text.lstrip(), re.I) is not None


def html_analysis(data):
    text = data.decode("utf-8")
    lines = text.splitlines(keepends=True)
    offsets = [0]
    for line in lines:
        offsets.append(offsets[-1] + len(line.encode("utf-8")))
    found = []
    events = []
    class Reader(HTMLParser):
        CDATA_CONTENT_ELEMENTS = ("script", "style", "title", "textarea", "xmp", "iframe", "noembed", "noframes")
        embedded = None
        raw_content = False
        def handle_starttag(self, tag, attrs):
            if tag == "plaintext":
                raise Refusal("HTML plaintext element is unsupported")
            self.raw_content = tag in self.CDATA_CONTENT_ELEMENTS and tag not in {"script", "style"}
            events.append(("start", self.get_starttag_text()))
            if tag in {"script", "style"}:
                kind = dict(attrs).get("type", "").lower()
                self.embedded = "css" if tag == "style" else "json" if "json" in kind or kind in {"importmap", "speculationrules"} else "javascript" if kind in {"", "module", "text/javascript", "application/javascript", "text/ecmascript"} else "unsupported"
        def handle_startendtag(self, tag, attrs):
            events.append(("empty", self.get_starttag_text()))
        def handle_endtag(self, tag):
            self.raw_content = False
            events.append(("end", tag))
            if tag in {"script", "style"}:
                self.embedded = None
        def handle_data(self, value):
            if self.raw_content:
                events.append(("rawdata", value))
                return
            if self.embedded is None and "<!--" in value:
                raise Refusal("unterminated HTML comment")
            if self.embedded and value.strip():
                if self.embedded == "unsupported":
                    raise Refusal("unsupported embedded script type")
                row, column = self.getpos()
                offset = offsets[row - 1] + len(lines[row - 1][:column].encode("utf-8"))
                if self.embedded != "json":
                    found.extend(tree_comments(value.encode("utf-8"), self.embedded, offset))
                events.append(("embedded", fingerprint(value.encode("utf-8"), self.embedded)))
            elif events and events[-1][0] == "data":
                events[-1] = ("data", events[-1][1] + value)
            else:
                events.append(("data", value))
        def handle_comment(self, value):
            row, column = self.getpos()
            start = offsets[row - 1] + len(lines[row - 1][:column].encode("utf-8"))
            match = re.match(rb"<!--[\s\S]*?--\s*>", data[start:])
            if match is None:
                raise Refusal("unsupported HTML comment delimiter")
            end = start + match.end()
            found.append(Comment(start, end, data[start:end], "html"))
        def handle_decl(self, value):
            events.append(("declaration", value))
        def handle_entityref(self, name):
            events.append(("entity", name))
        def handle_charref(self, name):
            events.append(("character", name))
        def handle_pi(self, value):
            raise Refusal("embedded processing instruction is unsupported")
        def unknown_decl(self, value):
            raise Refusal("unknown HTML declaration is unsupported")
    reader = Reader(convert_charrefs=False)
    reader.feed(text)
    reader.close()
    if any(event[0] == "data" and "<!--" in event[1] for event in events):
        raise Refusal("unterminated HTML comment")
    return found, tuple(events)


def transform_notebook(data, export_legal):
    notebook = json.loads(data)
    language = notebook.get("metadata", {}).get("kernelspec", {}).get("language", notebook.get("metadata", {}).get("language_info", {}).get("name", "python")).lower()
    language = {"python3": "python", "julia": "unsupported"}.get(language, language)
    if language not in set(LANGUAGES.values()) - {"notebook"}:
        raise Refusal(f"unsupported notebook language: {language}")
    found = []
    notices = []
    for index, cell in enumerate(notebook.get("cells", [])):
        if cell.get("cell_type") != "code":
            continue
        source = cell.get("source", [])
        original = "".join(source) if isinstance(source, list) else source
        try:
            output, cell_found, cell_notices = transform(original.encode("utf-8"), language, export_legal)
        except (ValueError, SyntaxError, tokenize.TokenError) as error:
            raise Refusal(f"notebook cell {index}: {error}") from error
        cell["source"] = output.decode("utf-8").splitlines(keepends=True) if isinstance(source, list) else output.decode("utf-8")
        found.extend(cell_found)
        notices.extend((line, f"Notebook cell {index}:\n{text}") for line, text in cell_notices)
    if not found:
        return data, found, notices
    return (json.dumps(notebook, indent=1, ensure_ascii=False) + "\n").encode("utf-8"), found, notices


def syntax_directive(comment, data=None):
    if comment.language == "dockerfile" and data is not None:
        offset = 0
        for line in data.splitlines(keepends=True):
            directive = re.fullmatch(rb"[ \t]*#[ \t]*(syntax|escape)[ \t]*=[ \t]*(\S[^\r\n]*)[\r\n]*", line, re.I)
            if directive is None:
                break
            if directive.group(1).lower() == b"escape" and directive.group(2).strip() not in {b"\\", b"`"}:
                raise Refusal("unsupported Docker escape parser directive")
            if offset <= comment.start < offset + len(line):
                return True
            offset += len(line)
    if comment.language in {"python", "bash", "javascript"} and comment.start == 0 and comment.text.startswith(b"#!"):
        return True
    if comment.language != "python" or data is None:
        return False
    lines = data.splitlines(keepends=True)
    first = lines[0] if lines else b""
    eligible = lines[:1] if first.strip() and not first.lstrip(b" \t\f").startswith(b"#") else lines[:2]
    offset = 0
    for line in eligible:
        if re.match(rb"^[ \t\f]*#.*?coding[:=][ \t]*([-_.a-zA-Z0-9]+)", line):
            return offset <= comment.start < offset + len(line)
        offset += len(line)
    return False


def mask(data):
    return b"".join(piece if index % 2 else b" " * len(piece) for index, piece in enumerate(re.split(rb"(\r\n|[\r\n]|\xe2\x80[\xa8\xa9])", data)))


def trim_comment_line_ends(original, output, found):
    touched = set()
    for comment in found:
        first = original[:comment.start].count(b"\n")
        last = first + comment.text.count(b"\n")
        touched.update(range(first, last + 1))
    lines = output.split(b"\n")
    for index in touched:
        if index < len(lines):
            ending = b"\r" if lines[index].endswith(b"\r") else b""
            lines[index] = lines[index].removesuffix(b"\r").rstrip(b" \t") + ending
    return b"\n".join(lines)



def replace(data, replacements):
    for start, end, replacement in sorted(replacements, reverse=True):
        data = data[:start] + replacement + data[end:]
    return data


def fingerprint(data, language):
    if language == "workflow":
        found = workflow_comments(data)
        output = replace(data, [(item.start, item.end, mask(item.text)) for item in found])
        return fingerprint(trim_comment_line_ends(data, output, found), "yaml")
    if language == "xml":
        return ET.tostring(ET.fromstring(data))
    if language == "doxygen":
        output = replace(data, [(item.start, item.end, b"") for item in comments(data, language)])
        return tuple(re.findall(rb'"(?:[^"\\]|\\.)*"|[^\s"]+', output))
    if language == "make":
        output = replace(data, [(item.start, item.end, b"") for item in make_comments(data)])
        return output
    if language == "sql":
        return sql_lex(data)[1]
    if language in {"ignore", "attributes", "systemd", "syncthing"}:
        output = replace(data, [(item.start, item.end, b"") for item in line_comments(data, language)])
        return tuple(line for line in output.splitlines() if line.strip())
    if language == "python":
        return ast.dump(ast.parse(data), include_attributes=False)
    if language == "html":
        return html_analysis(data)[1]
    tree = parse_tree(data, language)
    def visit(node):
        if node.type in COMMENT_TYPES:
            return None
        children = tuple(item for child in node.children if (item := visit(child)) is not None)
        return (node.type, children if node.children else node.text)
    return visit(tree.root_node)


def transform(data, language, export_legal=False):
    if language == "notebook":
        return transform_notebook(data, export_legal)
    found = [comment for comment in comments(data, language) if not syntax_directive(comment, data)]
    if not found:
        return data, found, []
    replacements = []
    notices = []
    groups = []
    for comment in sorted(found, key=lambda item: item.start):
        if groups and not data[groups[-1][-1].end:comment.start].strip():
            groups[-1].append(comment)
        else:
            groups.append([comment])
    for group in groups:
        if any(LEGAL.search(comment.text) for comment in group):
            line = data[:group[0].start].count(b"\n") + 1
            if not export_legal:
                raise Refusal(f"legal notice requires --notices export at line {line}")
            notices.append((line, data[group[0].start:group[-1].end].decode("utf-8")))
    for comment in found:
        replacement = b"" if comment.language in {"html", "xml"} or language == "make" else mask(comment.text)
        replacements.append((comment.start, comment.end, replacement))
    output = replace(data, replacements)
    if language not in {"html", "xml", "make"}:
        output = trim_comment_line_ends(data, output, found)
    before = data
    if fingerprint(before, language) != fingerprint(output, language):
        raise Refusal("non-comment syntax changed")
    if language == "toml" and tomllib.loads(before.decode("utf-8")) != tomllib.loads(output.decode("utf-8")):
        raise Refusal("TOML values changed")
    remaining = [comment for comment in comments(output, language) if not syntax_directive(comment, output)]
    if remaining:
        raise Refusal("transformation left comments")
    return output, found, notices


def tracked_sources(root):
    names = subprocess.check_output(["git", "-C", str(root), "ls-files", "-z"]).decode().split("\0")
    selected = []
    for name in names:
        if not name:
            continue
        path = root / name
        if path.is_symlink():
            selected.append(path)
            continue
        if not path.is_file():
            continue
        try:
            language_for(path, path.read_bytes())
        except Refusal:
            if path.suffix not in KNOWN_UNSUPPORTED and not path.read_bytes().startswith(b"#!"):
                continue
        selected.append(path)
    return selected


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("paths", nargs="*", type=Path)
    parser.add_argument("--repo", type=Path)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--notices", type=Path)
    args = parser.parse_args(argv)
    paths = sorted(set(args.paths + (tracked_sources(args.repo) if args.repo else [])))
    if not paths:
        parser.error("supply source paths or --repo")
    records = []
    prepared = []
    for path in paths:
        try:
            if path.is_symlink() or any(parent.is_symlink() for parent in path.absolute().parents):
                raise Refusal("symlink source or ancestor refused")
            original = path.read_bytes()
            language = language_for(path, original)
            output, found, notices = transform(original, language, args.notices is not None)
            record = {"path": str(path), "language": language, "comments": len(found), "rust_docs": sum(comment.doc for comment in found), "tool_directives": sum(tooling_directive(comment) for comment in found), "before": digest(original), "after": digest(output), "status": "comments" if found else "clean"}
            prepared.append((path, original, output, notices))
        except (ValueError, SyntaxError, tokenize.TokenError, OSError) as error:
            record = {"path": str(path), "status": "refused", "error": str(error)}
        records.append(record)
    refused = any(record["status"] == "refused" for record in records)
    if args.write and not refused:
        notice_sections = []
        for path, original, output, notices in prepared:
            if path.read_bytes() != original:
                raise Refusal(f"source changed during preflight: {path}")
            for line, text in notices:
                notice_sections.append(f"\nSource: {path}, original line {line}, SHA256 {digest(original)}\n\n{text}\n")
        if notice_sections:
            with args.notices.open("a", encoding="utf-8") as stream:
                stream.write("".join(notice_sections))
        for path, original, output, notices in prepared:
            if original != output:
                path.write_bytes(output)
        for record in records:
            record["status"] = "rewritten" if record["comments"] else "clean"
    print(json.dumps({"files": records, "write": args.write, "refused": refused}, indent=2))
    return 2 if refused else 1 if not args.write and any(record["comments"] for record in records) else 0


if __name__ == "__main__":
    sys.exit(main())
