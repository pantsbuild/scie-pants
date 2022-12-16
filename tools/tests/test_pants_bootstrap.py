# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import os
import subprocess


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
