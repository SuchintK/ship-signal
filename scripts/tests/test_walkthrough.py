"""Exercise the public CLI against disposable repositories and committed source."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


TOOL = Path(__file__).resolve().parents[1] / "walkthrough.py"
SOURCE = "first boundary\nold interior\nlast boundary\n"


class WalkthroughCLI(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "-q")
        for key, value in [
            ("user.name", "Walkthrough tests"),
            ("user.email", "tests@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.hooksPath", str(self.root / "unused-hooks")),
        ]:
            self.git("config", key, value)
        (self.root / "source.txt").write_text(SOURCE)
        (self.root / "README.md").write_text("# Guide\n\n## Local commands\n")
        self.old = self.commit()
        self.file = self.root / "docs/05-pr-walkthroughs/pr-1.html"
        self.file.parent.mkdir(parents=True)
        self.figure = self.cli(
            "snippet",
            "--source",
            self.old,
            "--path",
            "source.txt",
            "--start",
            "1",
            "--end",
            "3",
        ).stdout.strip()
        self.file.write_text(self.page(self.figure))

    def git(self, *args):
        return subprocess.check_output(
            ["git", "-C", str(self.root), *args], text=True
        ).strip()

    def commit(self):
        self.git("add", "--all")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def cli(self, *args, success=True):
        result = subprocess.run(
            [sys.executable, str(TOOL), *args],
            cwd=self.root,
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result

    def page(self, figure, extra=""):
        return (
            f"<!doctype html><html><head><title>Guide</title></head><body>"
            f"<p>Analyzed source SHA: {self.old}<br>Base SHA: {self.old}</p>"
            '<nav><a href="#flow">Flow</a></nav><section id="flow"><h2>Flow</h2>'
            "<p>Human explanation stays here.</p>"
            + figure
            + extra
            + '<a href="../../README.md#local-commands">Commands</a></section></body></html>'
        )

    def refresh(self, source, *args, success=True):
        return self.cli(
            "refresh",
            str(self.file),
            "--source",
            source,
            "--base",
            self.old,
            *args,
            success=success,
        )

    def changed_source(self, text):
        (self.root / "source.txt").write_text(text)
        return self.commit()

    def test_checks_existing_historical_source_without_requiring_head(self):
        self.changed_source("changed everywhere\n")
        self.cli("check")

    def test_uncommitted_source_edits_are_outside_the_snapshot(self):
        (self.root / "source.txt").write_text("uncommitted replacement\n")
        self.cli("check")
        result = self.cli(
            "snippet",
            "--source",
            "HEAD",
            "--path",
            "source.txt",
            "--start",
            "1",
            "--end",
            "3",
        )
        self.assertIn("old interior", result.stdout)
        self.assertNotIn("uncommitted replacement", result.stdout)

    def test_refresh_preview_leaves_file_unchanged_and_write_updates_inside_range(self):
        new = self.changed_source(
            "prefix\nfirst boundary\nnew interior\nlast boundary\n"
        )
        before = self.file.read_text()
        preview = self.refresh(new)
        self.assertIn("new interior", preview.stdout)
        self.assertEqual(self.file.read_text(), before)
        self.refresh(new, "--write")
        updated = self.file.read_text()
        self.assertIn('data-start="2" data-end="4"', updated)
        self.assertIn("Human explanation stays here.", updated)
        self.assertIn("Analyzed source SHA: " + new, updated)
        self.cli("check")

    def test_refresh_can_follow_a_uniquely_moved_block(self):
        new = self.changed_source("another section\n" + SOURCE)
        self.refresh(new, "--write")
        self.assertIn('data-start="2" data-end="4"', self.file.read_text())
        self.cli("check")

    def test_refresh_is_idempotent(self):
        self.refresh(self.old, "--write")
        first = self.file.read_bytes()
        self.refresh(self.old, "--write")
        self.assertEqual(self.file.read_bytes(), first)

    def test_boundary_change_refuses_to_write(self):
        new = self.changed_source(
            "changed first boundary\nold interior\nlast boundary\n"
        )
        before = self.file.read_bytes()
        result = self.refresh(new, "--write", success=False)
        self.assertIn("boundary changed", result.stderr)
        self.assertEqual(self.file.read_bytes(), before)

    def test_ambiguous_relocation_refuses_to_write(self):
        new = self.changed_source(SOURCE + "separator\n" + SOURCE)
        before = self.file.read_bytes()
        result = self.refresh(new, "--write", success=False)
        self.assertIn("multiple locations", result.stderr)
        self.assertEqual(self.file.read_bytes(), before)

    def test_unique_anchors_allow_changed_boundaries(self):
        self.file.write_text(
            self.page(
                self.figure.replace(
                    'data-end="3"',
                    'data-end="3" data-start-anchor="boundary" data-end-anchor="last boundary"',
                )
            )
        )
        new = self.changed_source(
            "first boundary renamed\nnew interior\nlast boundary\n"
        )
        # "boundary" is ambiguous, so the first refresh must not change the file.
        before = self.file.read_bytes()
        self.refresh(new, "--write", success=False)
        self.assertEqual(self.file.read_bytes(), before)
        self.file.write_text(
            self.file.read_text().replace(
                'data-start-anchor="boundary"', 'data-start-anchor="first"'
            )
        )
        self.refresh(new, "--write")
        self.assertIn("first boundary renamed", self.file.read_text())
        self.cli("check")

    def test_all_figures_are_resolved_before_writing(self):
        second = self.cli(
            "snippet",
            "--source",
            self.old,
            "--path",
            "source.txt",
            "--start",
            "1",
            "--end",
            "2",
        ).stdout
        self.file.write_text(self.page(self.figure + second))
        # The first range can move, but the second loses its ending boundary.
        new = self.changed_source(
            "prefix\nfirst boundary\nchanged interior\nlast boundary\n"
        )
        before = self.file.read_bytes()
        self.refresh(new, "--write", success=False)
        self.assertEqual(self.file.read_bytes(), before)

    def test_deleted_source_refuses_to_write(self):
        (self.root / "source.txt").unlink()
        new = self.commit()
        before = self.file.read_bytes()
        self.refresh(new, "--write", success=False)
        self.assertEqual(self.file.read_bytes(), before)

    def test_snippet_escapes_active_html_from_committed_source(self):
        new = self.changed_source('<script>alert("example")</script>\n')
        result = self.cli(
            "snippet",
            "--source",
            new,
            "--path",
            "source.txt",
            "--start",
            "1",
            "--end",
            "1",
        )
        self.assertIn("&lt;script&gt;", result.stdout)
        self.assertNotIn("<script>", result.stdout)
        self.file.write_text(self.page(result.stdout).replace(self.old, new))
        self.cli("check")

    def test_bad_excerpt_or_caption_fails_check(self):
        for figure in (
            self.figure.replace("old interior", "invented content"),
            self.figure.replace("lines 1–3", "lines 8–9"),
        ):
            with self.subTest(figure=figure):
                self.file.write_text(self.page(figure))
                self.cli("check", success=False)

    def test_invalid_html_and_offline_dependencies_fail_check(self):
        for extra in (
            "<script>bad()</script>",
            '<div onclick="bad()">x</div>',
            '<style>@import "https://example.invalid/style.css";</style>',
            '<p style="background:url(https://example.invalid/bg)">x</p>',
            '<div id="flow">duplicate</div>',
            "<div>unclosed",
            '<a href="javascript:bad()">bad</a>',
        ):
            with self.subTest(extra=extra):
                self.file.write_text(self.page(self.figure, extra))
                self.cli("check", success=False)

    def test_missing_links_and_navigation_fail_check(self):
        raw = self.page(self.figure)
        for modified in (
            raw.replace("#flow", "#missing"),
            raw.replace("#local-commands", "#absent"),
            raw.replace("../../README.md", "../../missing.md"),
            raw.replace('href="#flow"', 'href="../../README.md"'),
        ):
            with self.subTest(modified=modified):
                self.file.write_text(modified)
                self.cli("check", success=False)

    def test_valid_external_navigation_does_not_count_as_dependency(self):
        self.file.write_text(
            self.page(self.figure, '<a href="https://example.invalid/">Reference</a>')
        )
        self.cli("check")

    def test_html_target_anchors_are_elements_not_comment_text(self):
        (self.root / "linked.html").write_text(
            '<html><body><!-- id="fake" --><h1 id = "real">Title</h1></body></html>'
        )
        new = self.commit()
        self.file.write_text(
            self.page(
                self.figure, '<a href="../../linked.html#real">Linked</a>'
            ).replace(self.old, new)
        )
        self.cli("check")
        self.file.write_text(
            self.file.read_text().replace("linked.html#real", "linked.html#fake")
        )
        self.cli("check", success=False)

    def test_paths_cannot_escape_repository(self):
        self.cli(
            "snippet",
            "--source",
            self.old,
            "--path",
            "../outside",
            "--start",
            "1",
            "--end",
            "1",
            success=False,
        )
        self.file.write_text(
            self.page(self.figure, '<a href="../../../outside.md">Outside</a>')
        )
        self.cli("check", success=False)

    def test_invalid_ranges_and_missing_commits_fail(self):
        for start, end in [("0", "3"), ("3", "2"), ("1", "99")]:
            self.cli(
                "snippet",
                "--source",
                self.old,
                "--path",
                "source.txt",
                "--start",
                start,
                "--end",
                end,
                success=False,
            )
        self.cli(
            "snippet",
            "--source",
            "missing-ref",
            "--path",
            "source.txt",
            "--start",
            "1",
            "--end",
            "2",
            success=False,
        )

    def test_metadata_and_ancestry_are_checked(self):
        original = self.file.read_text()
        self.file.write_text(
            original.replace("Base SHA: " + self.old, "Base SHA: " + "0" * 40)
        )
        self.cli("check", success=False)
        self.file.write_text(
            original.replace("</body>", "<p>Base SHA: " + self.old + "</p></body>")
        )
        self.cli("check", success=False)
        self.file.write_text(original)
        new = self.changed_source("future version\n")
        self.file.write_text(
            original.replace("Base SHA: " + self.old, "Base SHA: " + new)
        )
        self.cli("check", success=False)

    def test_anchor_snippet_records_selection_for_future_refresh(self):
        result = self.cli(
            "snippet",
            "--source",
            self.old,
            "--path",
            "source.txt",
            "--start-anchor",
            "first boundary",
            "--end-anchor",
            "last boundary",
        )
        self.assertIn('data-start-anchor="first boundary"', result.stdout)
        self.file.write_text(self.page(result.stdout))
        self.cli("check")


if __name__ == "__main__":
    unittest.main()
