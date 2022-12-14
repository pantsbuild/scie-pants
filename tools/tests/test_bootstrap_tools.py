# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import os
import subprocess

import pytest
from testing import run_tool

from scie_pants import VERSION


def test_bootstrap_cache_key_no_env() -> None:
    with pytest.raises(subprocess.CalledProcessError):
        run_tool("bootstrap-tools", "bootstrap-cache-key")


PANTS_BOOTSTRAP_TOOLS_ENV = {**os.environ, "PANTS_BOOTSTRAP_TOOLS": f"{VERSION}"}


def test_bootstrap_cache_key_no_python_distribution_hash() -> None:
    with pytest.raises(subprocess.CalledProcessError):
        run_tool(
            "bootstrap-tools",
            "--pants-version",
            "2.14.0",
            "bootstrap-cache-key",
            env=PANTS_BOOTSTRAP_TOOLS_ENV,
        )


def test_bootstrap_cache_key_no_pants_version() -> None:
    with pytest.raises(subprocess.CalledProcessError):
        run_tool(
            "bootstrap-tools",
            "--python-distribution-hash",
            "abcd1234",
            "bootstrap-cache-key",
            env=PANTS_BOOTSTRAP_TOOLS_ENV,
        )


def test_bootstrap_cache_key() -> None:
    assert (
        "python_distribution_hash=abcd1234 pants_version=2.14.0"
        == run_tool(
            "bootstrap-tools",
            "--python-distribution-hash",
            "abcd1234",
            "--pants-version",
            "2.14.0",
            "bootstrap-cache-key",
            env=PANTS_BOOTSTRAP_TOOLS_ENV,
            stdout=subprocess.PIPE,
        )
        .stdout.decode()
        .strip()
    )


def test_bootstrap_version() -> None:
    assert (
        str(VERSION)
        == run_tool("bootstrap-tools", "bootstrap-version", stdout=subprocess.PIPE)
        .stdout.decode()
        .strip()
    )


def test_help() -> None:
    print(run_tool("bootstrap-tools", "help", stdout=subprocess.PIPE).stdout.decode())
