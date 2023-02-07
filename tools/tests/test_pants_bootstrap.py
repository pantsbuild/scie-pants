# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import os
import subprocess
import sys
from textwrap import dedent


def test_git_commit() -> None:
    # We configure this env var in pants.toml so that the test, which runs in a sandbox outside the
    # `.git` dir tree, can find the `.git` dir.
    build_root = os.environ["BUILDROOT"]

    git_commit = subprocess.run(
        args=["git", "rev-parse", "HEAD"],
        cwd=build_root,
        text=True,
        stdout=subprocess.PIPE,
        check=True,
    ).stdout.strip()

    # Confirm scie-pants parses `.pants.bootstrap` when present in the root of the repo.
    # We have one of these set up, and it exports the HEAD commit via GIT_COMMIT.
    assert git_commit == os.environ["GIT_COMMIT"]


def test_escaping() -> None:
    assert "+['aws-oidc', 'open']" == os.environ["PANTS_DOCKER_TOOLS"]


def test_terminal() -> None:
    columns = os.environ["COLUMNS"]
    lines = os.environ["LINES"]

    # N.B.: These tests only work both meaningfully and automatically when run by the package crate
    # test harness which exports the EXPECTED_* env vars.
    assert columns == os.environ.get("EXPECTED_COLUMNS", columns)
    assert lines == os.environ.get("EXPECTED_LINES", lines)
