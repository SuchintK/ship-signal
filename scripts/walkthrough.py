#!/usr/bin/env python3
"""Refresh and validate offline PR walkthroughs using committed Git source."""

import argparse
import difflib
import functools
import html
from html.parser import HTMLParser
from pathlib import Path, PurePosixPath
import posixpath
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit


class WalkthroughError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise WalkthroughError(message)


class Repository:
    def __init__(self, root):
        self.root = Path(root).resolve()

    def git(self, *args):
        result = subprocess.run(
            ["git", "-C", str(self.root), *args], text=True, capture_output=True
        )
        require(result.returncode == 0, result.stderr.strip() or "Git command failed")
        return result.stdout

    def commit(self, ref):
        return self.git(
            "rev-parse", "--verify", "--end-of-options", ref + "^{commit}"
        ).strip()

    @functools.lru_cache(maxsize=None)
    def read(self, commit, path):
        safe_path(path)
        return self.git("show", f"{commit}:{path}")

    def artifact(self, path):
        file = Path(path).resolve()
        require(
            file.is_relative_to(self.root), "walkthrough must be inside the repository"
        )
        require(file.is_file() and file.suffix == ".html", f"not an HTML file: {path}")
        return file, file.relative_to(self.root).as_posix()


def safe_path(path):
    require(
        path
        and not PurePosixPath(path).is_absolute()
        and ".." not in PurePosixPath(path).parts
        and "\\" not in path,
        f"path must stay inside the repository: {path}",
    )
    return path


def metadata(raw):
    values = []
    for label in ("Analyzed source SHA", "Base SHA"):
        matches = re.findall(r"\b" + label + r":\s*([0-9a-f]{40})\b", raw)
        require(
            len(matches) == 1,
            f"expected exactly one {label} followed by a full commit SHA",
        )
        values.append(matches[0])
    return tuple(values)


VOID = {"meta", "br", "hr", "img", "wbr", "area", "col", "source"}
BANNED = {
    "script",
    "iframe",
    "object",
    "embed",
    "link",
    "base",
    "form",
    "input",
    "button",
    "video",
    "audio",
    "foreignobject",
    "frame",
    "frameset",
}


class Page(HTMLParser):
    def __init__(self, raw):
        super().__init__(convert_charrefs=True)
        self.ids = set()
        self.links = []
        self.navlinks = []
        self.headings = []
        self.stack = []
        self.attrs = []
        self.figure = None
        self.figures = []
        self.in_code = False
        self.in_style = False
        self.styles = []
        self.offsets = [0]
        for line in raw.splitlines(keepends=True):
            self.offsets.append(self.offsets[-1] + len(line))
        self.feed(raw)
        self.close()
        require(not self.stack, "unclosed HTML tags: " + ", ".join(self.stack))

    def absolute_offset(self):
        line, column = self.getpos()
        return self.offsets[line - 1] + column

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        require(len(attrs) == len(attributes), f"duplicate attributes on <{tag}>")
        require(
            tag not in BANNED,
            f"active or external resource tag is unsupported: <{tag}>",
        )
        require(
            not any(key.startswith("on") for key in attrs),
            "event-handler attributes are unsupported",
        )
        require(
            not any(
                key in attrs
                for key in (
                    "src",
                    "srcset",
                    "poster",
                    "action",
                    "formaction",
                    "xlink:href",
                )
            ),
            "external resource attributes are unsupported",
        )
        if tag == "meta":
            require(
                "http-equiv" not in attrs,
                "HTTP-equivalent meta directives are unsupported",
            )
        if "style" in attrs:
            self.styles.append(attrs["style"])
        if "id" in attrs:
            require(
                attrs["id"] and attrs["id"] not in self.ids,
                f'duplicate or empty ID: {attrs["id"]}',
            )
            self.ids.add(attrs["id"])
        if "href" in attrs:
            require(tag == "a", "only navigation links may use href")
            self.links.append(attrs["href"])
            if "nav" in self.stack:
                self.navlinks.append(attrs["href"])
        if tag in ("h2", "h3", "h4", "h5", "h6"):
            section = next(
                (
                    a.get("id")
                    for t, a in reversed(list(zip(self.stack, self.attrs)))
                    if t == "section"
                ),
                None,
            )
            self.headings.append(attrs.get("id") or section)
        if tag == "figure" and "data-source" in attrs:
            require(self.figure is None, "nested source figures are unsupported")
            self.figure = {
                "attrs": attrs,
                "start": self.absolute_offset(),
                "code": [],
                "captions": [],
            }
        if self.figure and tag == "code" and self.stack[-1:] == ["pre"]:
            self.in_code = True
            self.figure["code_start"] = self.absolute_offset() + len(
                self.get_starttag_text()
            )
        if self.figure and tag == "figcaption":
            self.figure["caption_start"] = self.absolute_offset() + len(
                self.get_starttag_text()
            )
        if tag == "style":
            self.in_style = True
        if tag not in VOID:
            self.stack.append(tag)
            self.attrs.append(attrs)

    def handle_endtag(self, tag):
        require(
            bool(self.stack) and self.stack[-1] == tag,
            f"unbalanced closing tag: </{tag}>",
        )
        if self.figure and tag == "code" and self.in_code:
            self.in_code = False
            self.figure["code_end"] = self.absolute_offset()
        if self.figure and tag == "figcaption":
            self.figure["caption_end"] = self.absolute_offset()
        if tag == "figure" and self.figure:
            self.figure["end"] = self.absolute_offset() + len("</figure>")
            self.figures.append(self.figure)
            self.figure = None
        if tag == "style":
            self.in_style = False
        self.stack.pop()
        self.attrs.pop()

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag not in VOID:
            self.handle_endtag(tag)

    def handle_data(self, data):
        if self.figure and self.in_code:
            self.figure["code"].append(data)
        if self.figure and "figcaption" in self.stack:
            self.figure["captions"].append(data)
        if self.in_style:
            self.styles.append(data)


def numbered(lines, start, end):
    require(
        1 <= start <= end <= len(lines),
        f"invalid source range {start}–{end} for {len(lines)} lines",
    )
    return "\n".join(
        f"{i:3}" + ("  " + lines[i - 1] if lines[i - 1] else "")
        for i in range(start, end + 1)
    )


def source_range(attrs):
    try:
        return int(attrs["data-start"]), int(attrs["data-end"])
    except (KeyError, ValueError, TypeError) as error:
        raise WalkthroughError(
            "source figures need integer data-start and data-end attributes"
        ) from error


def markdown_ids(raw):
    result = set()
    for heading in re.findall(r"^#{1,6}\s+(.+?)\s*#*$", raw, re.M):
        heading = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", heading)
        heading = re.sub(r"<[^>]+>", "", html.unescape(heading))
        slug = re.sub(r"\s", "-", re.sub(r"[^\w\s-]", "", heading.lower()))
        candidate, count = slug, 0
        while candidate in result:
            count += 1
            candidate = f"{slug}-{count}"
        result.add(candidate)
    result.update(re.findall(r'\bid=["\']([^"\']+)["\']', raw))
    return result


def html_ids(raw):
    class IDs(HTMLParser):
        def __init__(self):
            super().__init__()
            self.values = set()

        def handle_starttag(self, tag, attrs):
            value = dict(attrs).get("id")
            if value:
                self.values.add(value)

    parser = IDs()
    parser.feed(raw)
    return parser.values


def validate(repo, file, raw):
    source, base = metadata(raw)
    require(
        repo.commit(source) == source and repo.commit(base) == base,
        "metadata must identify commits",
    )
    repo.git("merge-base", "--is-ancestor", base, source)
    page = Page(raw)
    require(page.figures, "walkthrough must contain at least one source figure")
    for style in page.styles:
        require(isinstance(style, str), "style attributes must have a value")
        require(
            not re.search(
                r"url\s*\(|@import|expression\s*\(", html.unescape(style), re.I
            ),
            "CSS resource imports and executable expressions are unsupported",
        )
    for heading in page.headings:
        require(
            heading and "#" + heading in page.navlinks,
            f"heading needs a navigation link: {heading}",
        )
    for figure in page.figures:
        attrs = figure["attrs"]
        path = safe_path(attrs["data-source"])
        start, end = source_range(attrs)
        expected = numbered(repo.read(source, path).splitlines(), start, end)
        if "data-start-anchor" in attrs or "data-end-anchor" in attrs:
            require(
                anchor_range(
                    repo.read(source, path).splitlines(),
                    attrs.get("data-start-anchor"),
                    attrs.get("data-end-anchor"),
                )
                == (start, end),
                f"{path}: source anchors do not match the recorded line range",
            )
        require(
            "".join(figure["code"]) == expected,
            f"{path}:{start}–{end}: excerpt differs from recorded source",
        )
        caption = "".join(figure["captions"])
        require(
            path in caption and f"lines {start}–{end}" in caption,
            f"{path}: caption must identify its path and line range",
        )
        require(
            "code_start" in figure and "caption_start" in figure,
            f"{path}: source figure needs a caption and pre/code block",
        )
    for href in page.links:
        url = urlsplit(href)
        require(
            url.scheme in ("", "https", "http") and not (url.netloc and not url.scheme),
            f"unsupported navigation URL: {href}",
        )
        if url.scheme:
            continue
        require(not url.path.startswith("/"), f"local links must be relative: {href}")
        if not url.path:
            require(unquote(url.fragment) in page.ids, f"missing local anchor: {href}")
            continue
        target = safe_path(
            posixpath.normpath(
                posixpath.join(str(PurePosixPath(file).parent), unquote(url.path))
            )
        )
        if target == file:
            content = raw
        else:
            content = repo.read(source, target)
        if url.fragment:
            if target.endswith(".html"):
                ids = html_ids(content)
            else:
                ids = markdown_ids(content)
            require(unquote(url.fragment) in ids, f"missing target anchor: {href}")
    return page


def anchor_range(lines, start_anchor, end_anchor):
    positions = []
    for anchor in (start_anchor, end_anchor):
        matches = [i + 1 for i, line in enumerate(lines) if anchor and anchor in line]
        require(
            len(matches) == 1, f"anchor must match exactly one source line: {anchor!r}"
        )
        positions.append(matches[0])
    numbered(lines, *positions)
    return tuple(positions)


def moved_range(old, new, start, end, attrs):
    if "data-start-anchor" in attrs or "data-end-anchor" in attrs:
        return anchor_range(
            new, attrs.get("data-start-anchor"), attrs.get("data-end-anchor")
        )
    if old == new:
        return start, end
    block = old[start - 1 : end]
    matches = [
        i + 1
        for i in range(len(new) - len(block) + 1)
        if new[i : i + len(block)] == block
    ]
    require(
        len(matches) <= 1,
        "snippet now matches multiple locations; use unique data-start-anchor and data-end-anchor attributes",
    )
    if matches:
        return matches[0], matches[0] + len(block) - 1
    old_matches = sum(
        old[i : i + len(block)] == block for i in range(len(old) - len(block) + 1)
    )
    require(old_matches == 1, "old snippet is ambiguous; use unique source anchors")
    positions = []
    operations = difflib.SequenceMatcher(None, old, new, autojunk=False).get_opcodes()
    for boundary in (start - 1, end - 1):
        mapped = [
            j1 + boundary - i1 + 1
            for tag, i1, i2, j1, _ in operations
            if tag == "equal" and i1 <= boundary < i2
        ]
        require(
            len(mapped) == 1,
            "snippet boundary changed or disappeared; reselect the range or add unique source anchors",
        )
        positions.append(mapped[0])
    numbered(new, *positions)
    return tuple(positions)


def refresh(repo, file, raw, source, base):
    page = validate(repo, file, raw)
    old_source, _ = metadata(raw)
    source, base = repo.commit(source), repo.commit(base)
    edits = []
    for figure in page.figures:
        attrs = figure["attrs"]
        old = repo.read(old_source, attrs["data-source"]).splitlines()
        new = repo.read(source, attrs["data-source"]).splitlines()
        start, end = moved_range(old, new, *source_range(attrs), attrs)
        generated = raw[figure["start"] : figure["end"]]
        for name, value in (("data-start", start), ("data-end", end)):
            generated = re.sub(
                r"(\b" + name + r'=["\'])\d+(["\'])',
                lambda m: m[1] + str(value) + m[2],
                generated,
                count=1,
            )
        generated = re.sub(
            r"<figcaption\b[^>]*>.*?</figcaption>",
            lambda _: "<figcaption>"
            + html.escape(f'{attrs["data-source"]} · lines {start}–{end}')
            + " <span>exact source excerpt</span></figcaption>",
            generated,
            count=1,
            flags=re.S,
        )
        generated = re.sub(
            r"<pre><code>.*?</code></pre>",
            lambda _: "<pre><code>"
            + html.escape(numbered(new, start, end))
            + "</code></pre>",
            generated,
            count=1,
            flags=re.S,
        )
        edits.append((figure["start"], figure["end"], generated))
    for start, end, value in reversed(edits):
        raw = raw[:start] + value + raw[end:]
    for label, value in (("Analyzed source SHA", source), ("Base SHA", base)):
        raw = re.sub(
            r"(" + label + r":\s*)[0-9a-f]{40}", lambda m: m[1] + value, raw, count=1
        )
    validate(repo, file, raw)
    return raw


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser(
        "check", help="validate pinned source excerpts and offline HTML"
    )
    check.add_argument(
        "files", nargs="*", help="defaults to all docs/05-pr-walkthroughs/pr-*.html"
    )
    update = commands.add_parser(
        "refresh", help="preview mechanical updates; --write applies them"
    )
    update.add_argument("file")
    update.add_argument(
        "--source", required=True, help="committed source ref to describe"
    )
    update.add_argument(
        "--base", required=True, help="base commit ref for the walkthrough"
    )
    update.add_argument("--write", action="store_true")
    snippet = commands.add_parser(
        "snippet", help="print an escaped source figure for a new walkthrough"
    )
    snippet.add_argument("--source", required=True)
    snippet.add_argument("--path", required=True)
    snippet.add_argument("--start", type=int)
    snippet.add_argument("--end", type=int)
    snippet.add_argument("--start-anchor")
    snippet.add_argument("--end-anchor")
    args = parser.parse_args(argv)
    root = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], text=True, capture_output=True
    )
    require(root.returncode == 0, "run inside a Git repository")
    repo = Repository(root.stdout.strip())
    if args.command == "snippet":
        source = repo.commit(args.source)
        lines = repo.read(source, args.path).splitlines()
        if args.start_anchor or args.end_anchor:
            require(
                args.start is None and args.end is None,
                "choose line numbers or anchors, not both",
            )
            start, end = anchor_range(lines, args.start_anchor, args.end_anchor)
            anchors = f' data-start-anchor="{html.escape(args.start_anchor, quote=True)}" data-end-anchor="{html.escape(args.end_anchor, quote=True)}"'
        else:
            require(
                args.start is not None and args.end is not None,
                "provide --start and --end, or both anchor options",
            )
            start, end, anchors = args.start, args.end, ""
        code = html.escape(numbered(lines, start, end))
        print(
            f'<figure data-source="{html.escape(args.path, quote=True)}" data-start="{start}" data-end="{end}"{anchors}><figcaption>{html.escape(args.path)} · lines {start}–{end} <span>exact source excerpt</span></figcaption><pre><code>{code}</code></pre></figure>'
        )
    elif args.command == "check":
        files = args.files or sorted(
            (repo.root / "docs/05-pr-walkthroughs").glob("pr-*.html")
        )
        require(files, "no walkthroughs found")
        for path in files:
            artifact, file = repo.artifact(path)
            page = validate(repo, file, artifact.read_text())
            print(f"{file}: OK ({len(page.figures)} excerpts, {len(page.links)} links)")
    else:
        artifact, file = repo.artifact(args.file)
        original = artifact.read_text()
        updated = refresh(repo, file, original, args.source, args.base)
        if args.write:
            require(
                artifact.read_text() == original,
                "walkthrough changed during refresh; inspect it before retrying",
            )
            artifact.write_text(updated)
            print(f"{file}: refreshed; review prose and test claims before publishing")
        else:
            sys.stdout.writelines(
                difflib.unified_diff(
                    original.splitlines(True),
                    updated.splitlines(True),
                    fromfile="a/" + file,
                    tofile="b/" + file,
                )
            )
            print(
                "Preview only. Review prose and test claims; use --write to apply.",
                file=sys.stderr,
            )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (WalkthroughError, OSError, UnicodeError, ValueError) as error:
        print(f"walkthrough: {error}", file=sys.stderr)
        sys.exit(1)
