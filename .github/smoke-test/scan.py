"""Scan upstream repositories using their own config or a repository fallback."""

from __future__ import annotations

import argparse
import csv
import hashlib
import os
import re
import shlex
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

import tomllib

FIELDS = (
    "repo",
    "failure_type",
    "file",
    "log_path",
    "report_path",
    "repo_sha",
    "panache_sha",
    "panache_version",
    "idempotency_input_path",
    "idempotency_once_path",
    "idempotency_twice_path",
    "fallback_config_path",
    "reproduction_path",
)
EXTENSIONS = ("*.md", "*.qmd", "*.Rmd", "*.ipynb")


@dataclass(frozen=True)
class Repository:
    repo: str
    fallback_config: Path

    @property
    def url(self) -> str:
        if self.repo.startswith("https://"):
            return self.repo
        return f"https://github.com/{self.repo}.git"


def load_repositories(manifest: Path) -> list[Repository]:
    with manifest.open("rb") as stream:
        data = tomllib.load(stream)
    if set(data) != {"repositories"} or not data["repositories"]:
        raise ValueError(f"{manifest}: expected a nonempty repositories array")
    configs_dir = (manifest.parent / "configs").resolve()
    repositories = []
    seen = set()
    for entry in data["repositories"]:
        if set(entry) - {"repo", "fallback-config"}:
            raise ValueError(f"{manifest}: unknown repository settings: {entry}")
        repo = entry["repo"]
        if not repo or any(character.isspace() for character in repo):
            raise ValueError(f"{manifest}: invalid repository: {repo!r}")
        if not repo.startswith("https://") and len(repo.split("/")) != 2:
            raise ValueError(f"{manifest}: expected owner/repo or an HTTPS URL: {repo}")
        if repo in seen:
            raise ValueError(f"{manifest}: duplicate repository: {repo}")
        seen.add(repo)
        fallback = (
            manifest.parent / entry.get("fallback-config", "configs/default.toml")
        ).resolve()
        if not fallback.is_relative_to(configs_dir):
            raise ValueError(
                f"{manifest}: fallback config must be inside {configs_dir}"
            )
        with fallback.open("rb") as stream:
            tomllib.load(stream)
        repositories.append(
            Repository(repo, fallback.relative_to(manifest.parent.resolve()))
        )
    return repositories


def git(*arguments: str, cwd: Path | None = None) -> bytes:
    return subprocess.check_output(["git", *arguments], cwd=cwd)


def clone_repository(repository: Repository, repo_dir: Path) -> None:
    if repo_dir.exists() or repo_dir.is_symlink():
        raise FileExistsError(f"Checkout path already exists: {repo_dir}")
    for attempt in range(1, 4):
        try:
            subprocess.run(
                ["git", "clone", "--depth", "1", repository.url, str(repo_dir)],
                check=True,
            )
            return
        except subprocess.CalledProcessError:
            # A failed clone can leave files that prevent the next attempt.
            if repo_dir.exists():
                shutil.rmtree(repo_dir)
            if attempt == 3:
                raise
            delay = 5 * attempt
            print(
                f"warning: clone of {repository.url} failed (attempt {attempt}/3); "
                f"retrying in {delay} seconds",
                flush=True,
            )
            time.sleep(delay)


def run_panache(
    panache: Path,
    arguments: list[str],
    repo_dir: Path,
    config: Path,
    output: Path,
) -> bool:
    with output.open("w") as stream:
        result = subprocess.run(
            [str(panache), "debug", "format", "--checks", "all", *arguments],
            cwd=repo_dir,
            env=os.environ | {"PANACHE_CONFIG": str(config)},
            stdout=stream,
            stderr=subprocess.STDOUT,
            check=False,
        )
    return result.returncode == 0


def reproduction(
    repository: Repository, repo_sha: str, rel_file: str, explicit_config: bool
) -> str:
    config = '"$scan_results_dir"/' + shlex.quote(repository.fallback_config.as_posix())
    directory = repository.url.rstrip("/").rsplit("/", 1)[-1].removesuffix(".git")
    arguments = "--config " + config + " " if explicit_config else ""
    return "\n".join(
        (
            'scan_results_dir="$(pwd)"',
            shlex.join(["git", "clone", repository.url]),
            shlex.join(["cd", directory]),
            shlex.join(["git", "checkout", repo_sha]),
            (
                f"PANACHE_CONFIG={config} panache debug format --checks all "
                f"{arguments}--report -- {shlex.quote(rel_file)}"
            ),
            "",
        )
    )


def scan(
    manifest: Path,
    panache: Path,
    results_dir: Path,
    repos_dir: Path,
    panache_sha: str,
    panache_version: str,
) -> None:
    repositories = load_repositories(manifest)
    if repos_dir.is_relative_to(results_dir):
        raise ValueError("Upstream checkouts must be outside the results directory")
    logs_dir = results_dir / "logs"
    logs_dir.mkdir(parents=True, exist_ok=True)
    repos_dir.mkdir(parents=True, exist_ok=True)
    # Preserve relative extend paths, and use the archived configs during the scan
    # so the artifact contains exactly the fallback files used by Panache.
    shutil.copytree(
        manifest.parent / "configs", results_dir / "configs", dirs_exist_ok=True
    )
    with (results_dir / "failures.tsv").open("w", newline="") as stream:
        failures = csv.DictWriter(
            stream, fieldnames=FIELDS, delimiter="\t", lineterminator="\n"
        )
        failures.writeheader()
        for repository in repositories:
            repo_key = hashlib.sha256(repository.repo.encode()).hexdigest()
            repo_dir = repos_dir / repo_key
            clone_repository(repository, repo_dir)
            repo_sha = git("rev-parse", "HEAD", cwd=repo_dir).decode().strip()
            config = results_dir / repository.fallback_config
            files = git("ls-files", "-z", "--", *EXTENSIONS, cwd=repo_dir).split(b"\0")
            for file in filter(None, files):
                rel_file = os.fsdecode(file)
                file_key = hashlib.sha256(
                    f"{repository.repo}:{rel_file}".encode()
                ).hexdigest()
                log = logs_dir / f"{file_key}.log"
                report = logs_dir / f"{file_key}.report.md"
                passes = logs_dir / f"{file_key}.passes"
                dump_arguments = [
                    "--dump-dir",
                    str(passes),
                ]
                # Notebook dumps can include large embedded outputs. The CLI
                # still writes the relevant passes when a notebook check fails.
                if Path(rel_file).suffix != ".ipynb":
                    dump_arguments.append("--dump-passes")
                dump_arguments.extend(["--", rel_file])
                config_arguments: list[str] = []
                if run_panache(panache, dump_arguments, repo_dir, config, log):
                    continue
                # Invalid upstream configs are retried with the same repository
                # fallback so configuration migrations do not become regressions.
                if "invalid config" in log.read_text(errors="replace"):
                    config_arguments = ["--config", str(config)]
                    if run_panache(
                        panache,
                        config_arguments + dump_arguments,
                        repo_dir,
                        config,
                        log,
                    ):
                        print(
                            f"notice: {repository.repo} has an invalid panache config; "
                            f"{rel_file} passed with the fallback config",
                            flush=True,
                        )
                        continue
                run_panache(
                    panache,
                    config_arguments + ["--report", "--", rel_file],
                    repo_dir,
                    config,
                    report,
                )
                reproduce = logs_dir / f"{file_key}.reproduce.sh"
                reproduce.write_text(
                    reproduction(repository, repo_sha, rel_file, bool(config_arguments))
                )
                row = {
                    "repo": repository.repo,
                    "file": rel_file,
                    "log_path": log.relative_to(results_dir).as_posix(),
                    "report_path": report.relative_to(results_dir).as_posix(),
                    "repo_sha": repo_sha,
                    "panache_sha": panache_sha,
                    "panache_version": panache_version,
                    "fallback_config_path": repository.fallback_config.as_posix(),
                    "reproduction_path": reproduce.relative_to(results_dir).as_posix(),
                }
                safe_file = re.sub(r"[^a-zA-Z0-9._-]", "_", rel_file)
                for name in ("input", "once", "twice"):
                    artifact = passes / f"{safe_file}.idempotency.{name}.txt"
                    if artifact.is_file():
                        row[f"idempotency_{name}_path"] = artifact.relative_to(
                            results_dir
                        ).as_posix()
                types = re.findall(
                    r"^Debug check failed \((idempotency|losslessness)\) in ",
                    log.read_text(errors="replace"),
                    re.MULTILINE,
                )
                for failure_type in types or ["unknown"]:
                    failures.writerow(row | {"failure_type": failure_type})
                stream.flush()


def main() -> None:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest", type=Path, default=Path(__file__).with_name("repositories.toml")
    )
    parser.add_argument(
        "--panache-bin", type=Path, default=root / "target/release/panache"
    )
    parser.add_argument("--results-dir", required=True, type=Path)
    parser.add_argument("--repos-dir", required=True, type=Path)
    args = parser.parse_args()
    panache = args.panache_bin.resolve()
    version = subprocess.check_output(
        [str(panache), "--version"], text=True
    ).splitlines()[0]
    scan(
        args.manifest.resolve(),
        panache,
        args.results_dir.resolve(),
        args.repos_dir.resolve(),
        git("rev-parse", "HEAD", cwd=root).decode().strip(),
        version,
    )


if __name__ == "__main__":
    main()
