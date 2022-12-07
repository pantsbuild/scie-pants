# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import sys
from typing import NoReturn

from colors import green, red, yellow


def log(message: str) -> None:
    print(message, file=sys.stderr)


def info(message: str) -> None:
    log(green(message))


def warn(message: str) -> None:
    log(yellow(message))


def fatal(message: str) -> NoReturn:
    sys.exit(red(message))
