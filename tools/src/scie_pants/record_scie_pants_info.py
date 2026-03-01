# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import os
import subprocess
import sys
from argparse import ArgumentParser
from pathlib import Path
from typing import NoReturn

from scie_pants.log import fatal, init_logging


def main() -> NoReturn:
    parser = ArgumentParser()
    parser.add_argument(
        "--base-dir",
        type=Path,
        required=True,
        help="The base directory of this scie's bindings",
    )
    parser.add_argument(
        "--scie",
        required=True,
        help="The path of the current scie executable.",
    )
    options = parser.parse_args()

    # N.B.: This installs an excepthook that gracefully handles uncaught exceptions; so any raises
    # or uncaught exceptions below here are clean ways to exit non-zero with useful console output.
    init_logging(base_dir=options.base_dir, log_name="record-scie-pants-info")

    env_file = os.environ.get("SCIE_BINDING_ENV")
    if not env_file:
        fatal("Expected SCIE_BINDING_ENV to be set in the environment")

    version = subprocess.run(
        args=[options.scie],
        env={**os.environ, "PANTS_BOOTSTRAP_VERSION": "report"},
        stdout=subprocess.PIPE,
        text=True,
        check=True,
    ).stdout.strip()
    with open(env_file, "a") as fp:
        print(f"VERSION={version}", file=fp)

    sys.exit(0)


if __name__ == "__main__":
    main()
