"""Offline tests for scan configuration, artifacts, and reproduction commands."""

from __future__ import annotations

import csv
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import scan


class ScanTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.manifest = self.root / "repositories.toml"
        self.configs = self.root / "configs"
        self.configs.mkdir()
        (self.configs / "default.toml").write_text("# Built-in defaults.\n")
        (self.configs / "base.toml").write_text('[format]\nwrap = "preserve"\n')
        (self.configs / "hugo.toml").write_text(
            'extend = "base.toml"\nflavor = "hugo"\n'
        )
        self.manifest.write_text(
            '[[repositories]]\nrepo = "owner/site"\n'
            'fallback-config = "configs/hugo.toml"\n'
        )
        self.results = self.root / "results"
        self.repos = self.root / "repos"
        self.panache = self.root / "panache"
        self.calls: list[tuple[list[str], dict]] = []
        self.clones: list[str] = []

    def run_scan(
        self,
        outcomes: list[tuple[int, str]],
        clone_codes: list[int] | None = None,
    ) -> list[dict]:
        pending = iter(outcomes)
        clones = iter(clone_codes or [])

        def run(command: list[str], **kwargs) -> subprocess.CompletedProcess:
            if command[:2] == ["git", "clone"]:
                self.clones.append(command[4])
                checkout = Path(command[5])
                checkout.mkdir()
                code = next(clones, 0)
                if code:
                    (checkout / "partial-clone").write_text("incomplete")
                    raise subprocess.CalledProcessError(code, command)
                return subprocess.CompletedProcess(command, 0)
            self.calls.append((command, kwargs))
            code, output = next(pending)
            kwargs["stdout"].write(output)
            if "--dump-dir" in command and "idempotency" in output:
                passes = Path(command[command.index("--dump-dir") + 1])
                passes.mkdir(exist_ok=True)
                name = re.sub(r"[^a-zA-Z0-9._-]", "_", command[-1])
                for stage in ("input", "once", "twice"):
                    (passes / f"{name}.idempotency.{stage}.txt").write_text(stage)
            return subprocess.CompletedProcess(command, code)

        def git(*arguments, cwd=None) -> bytes:
            if arguments == ("rev-parse", "HEAD"):
                return b"upstream-sha\n"
            self.assertEqual(arguments, ("ls-files", "-z", "--", *scan.EXTENSIONS))
            return b"content/a page.md\0"

        with (
            patch.object(scan.subprocess, "run", side_effect=run),
            patch.object(scan, "git", side_effect=git),
        ):
            scan.scan(
                self.manifest,
                self.panache,
                self.results,
                self.repos,
                "panache-sha",
                "panache test-version",
            )
        self.assertEqual(list(pending), [], "unused mocked Panache results")
        with (self.results / "failures.tsv").open(newline="") as stream:
            return list(csv.DictReader(stream, delimiter="\t"))

    def test_clone_retries_clean_partial_checkouts_and_resume_scan(self) -> None:
        with patch("time.sleep") as sleep:
            rows = self.run_scan([(0, "passed")], clone_codes=[128, 128, 0])
        self.assertEqual(rows, [])
        self.assertEqual(len(self.clones), 3)
        self.assertEqual([args[0] for args, _ in sleep.call_args_list], [5, 10])
        self.assertEqual(list(self.repos.rglob("partial-clone")), [])

    def test_clone_exhaustion_preserves_prior_failure_records(self) -> None:
        with self.manifest.open("a") as stream:
            stream.write('\n[[repositories]]\nrepo = "owner/unavailable"\n')
        with (
            patch("time.sleep") as sleep,
            self.assertRaises(subprocess.CalledProcessError),
        ):
            self.run_scan(
                [
                    (1, "Debug check failed (idempotency) in content/a page.md\n"),
                    (1, "regression report"),
                ],
                clone_codes=[0, 128, 128, 128],
            )
        self.assertEqual(len(self.clones), 4)
        self.assertEqual(sleep.call_count, 2)
        with (self.results / "failures.tsv").open(newline="") as stream:
            rows = list(csv.DictReader(stream, delimiter="\t"))
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["repo"], "owner/site")
        self.assertEqual(rows[0]["failure_type"], "idempotency")
        self.assertEqual(
            (self.results / rows[0]["report_path"]).read_text(), "regression report"
        )
        self.assertTrue((self.results / "configs/hugo.toml").is_file())

    def test_clone_does_not_modify_existing_checkout(self) -> None:
        checkout = self.root / "existing"
        checkout.mkdir()
        marker = checkout / "keep"
        marker.write_text("existing checkout")
        repository = scan.Repository("owner/site", Path("configs/default.toml"))
        with (
            patch.object(scan.subprocess, "run") as run,
            self.assertRaises(FileExistsError),
        ):
            scan.clone_repository(repository, checkout)
        run.assert_not_called()
        self.assertEqual(marker.read_text(), "existing checkout")

    def test_scan_preserves_urls_and_archives_fallbacks_and_failures(self) -> None:
        url = "https://gitlab.example.org/group/subgroup/site.git"
        with self.manifest.open("a") as stream:
            stream.write(f'\n[[repositories]]\nrepo = "{url}"\n')
        rows = self.run_scan(
            [
                (
                    1,
                    (
                        "Debug check failed (idempotency) in content/a page.md\n"
                        "Debug check failed (losslessness) in content/a page.md\n"
                    ),
                ),
                (
                    1,
                    (
                        "### 1. `content/a page.md` (idempotency)\n"
                        "### 2. `content/a page.md` (losslessness)\n"
                    ),
                ),
                (0, "passed"),
            ]
        )
        self.assertEqual(self.clones, ["https://github.com/owner/site.git", url])
        self.assertEqual(
            [row["failure_type"] for row in rows], ["idempotency", "losslessness"]
        )
        for row in rows:
            self.assertEqual(row["repo"], "owner/site")
            self.assertEqual(row["file"], "content/a page.md")
            self.assertEqual(row["repo_sha"], "upstream-sha")
            self.assertEqual(row["fallback_config_path"], "configs/hugo.toml")
            for key in (
                "idempotency_input_path",
                "idempotency_once_path",
                "idempotency_twice_path",
            ):
                self.assertTrue((self.results / row[key]).is_file(), key)
            self.assertTrue((self.results / row["reproduction_path"]).is_file())
        self.assertEqual(
            (self.results / "configs/base.toml").read_text(),
            (self.configs / "base.toml").read_text(),
        )
        for command, kwargs in self.calls[:2]:
            self.assertNotIn("--config", command)
            self.assertEqual(
                kwargs["env"]["PANACHE_CONFIG"], str(self.results / "configs/hugo.toml")
            )
        self.assertEqual(
            self.calls[2][1]["env"]["PANACHE_CONFIG"],
            str(self.results / "configs/default.toml"),
        )
        self.assertFalse(any(path.name == ".git" for path in self.results.rglob("*")))

    def test_document_text_does_not_classify_failures(self) -> None:
        rows = self.run_scan(
            [
                (
                    1,
                    (
                        "Debug check failed (idempotency) in content/a page.md\n"
                        "- Use lossless compression.\n"
                    ),
                ),
                (
                    1,
                    (
                        "### 1. `content/a page.md` (idempotency)\n"
                        "+ The losslessness and idempotency checks are documented here.\n"
                    ),
                ),
            ]
        )
        self.assertEqual([row["failure_type"] for row in rows], ["idempotency"])

    def test_invalid_upstream_config_retries_with_repository_fallback(self) -> None:
        rows = self.run_scan([(1, "Error: invalid config panache.toml"), (0, "passed")])
        self.assertEqual(rows, [])
        initial, retry = [command for command, _ in self.calls]
        self.assertNotIn("--config", initial)
        self.assertEqual(
            retry[retry.index("--config") + 1], str(self.results / "configs/hugo.toml")
        )

    def test_persistent_failure_keeps_explicit_config_in_report_and_reproduction(
        self,
    ) -> None:
        rows = self.run_scan(
            [(1, "invalid config"), (1, "unknown failure"), (1, "unknown failure")]
        )
        self.assertEqual([row["failure_type"] for row in rows], ["unknown"])
        for command, _ in self.calls[1:]:
            self.assertEqual(
                command[command.index("--config") + 1],
                str(self.results / "configs/hugo.toml"),
            )
        script = (self.results / rows[0]["reproduction_path"]).read_text()
        self.assertIn('--config "$scan_results_dir"/configs/hugo.toml', script)

    def test_checkout_inside_artifacts_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "outside the results directory"):
            scan.scan(
                self.manifest,
                self.panache,
                self.results,
                self.results / "repos",
                "sha",
                "version",
            )

    def test_manifest_rejects_duplicate_repos_and_unknown_settings(self) -> None:
        for extra, message in [
            ('\n[[repositories]]\nrepo = "owner/site"\n', "duplicate repository"),
            ('fallback-confgi = "configs/hugo.toml"\n', "unknown repository settings"),
        ]:
            with self.subTest(message=message):
                self.manifest.write_text(
                    '[[repositories]]\nrepo = "owner/site"\n' + extra
                )
                with self.assertRaisesRegex(ValueError, message):
                    scan.load_repositories(self.manifest)

    def test_checked_in_manifest_and_configs_load(self) -> None:
        repositories = scan.load_repositories(
            Path(__file__).with_name("repositories.toml")
        )
        self.assertGreater(len(repositories), 0)
        hugo = next(
            repository
            for repository in repositories
            if repository.repo == "gohugoio/hugoDocs"
        )
        self.assertEqual(hugo.fallback_config, Path("configs/hugo.toml"))

    def test_reproduction_uses_relocated_config_and_quotes_filenames(self) -> None:
        tools = self.root / "tools"
        tools.mkdir()
        trace = self.root / "trace.jsonl"
        stub = f"""#!{sys.executable}
import json
import os
from pathlib import Path
import sys
args = sys.argv[1:]
with open(os.environ['SMOKE_TRACE'], 'a') as stream:
    stream.write(json.dumps({{'args': args, 'config': os.environ.get('PANACHE_CONFIG')}}) + '\\n')
if args[0] == 'clone':
    Path('site').mkdir()
"""
        for name in ("git", "panache"):
            executable = tools / name
            executable.write_text(stub)
            executable.chmod(0o755)
        for explicit in (False, True):
            with self.subTest(explicit=explicit):
                directory = self.root / f"extracted artifact {explicit}"
                directory.mkdir()
                config = directory / "configs/hugo.toml"
                config.parent.mkdir()
                config.write_text('flavor = "hugo"\n')
                repository = scan.Repository(
                    "https://gitlab.example.org/group/subgroup/site.git",
                    Path("configs/hugo.toml"),
                )
                filename = "content/a 'quoted' page.md"
                subprocess.run(
                    [
                        "bash",
                        "-eu",
                        "-c",
                        scan.reproduction(repository, "sha", filename, explicit),
                    ],
                    cwd=directory,
                    env=os.environ
                    | {
                        "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                        "SMOKE_TRACE": str(trace),
                    },
                    check=True,
                )
                invocation = json.loads(trace.read_text().splitlines()[-1])
                self.assertEqual(invocation["config"], str(config))
                self.assertEqual(invocation["args"][-1], filename)
                self.assertEqual("--config" in invocation["args"], explicit)


if __name__ == "__main__":
    unittest.main()
