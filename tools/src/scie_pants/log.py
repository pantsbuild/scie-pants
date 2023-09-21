# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import logging
import sys
from logging.handlers import RotatingFileHandler
from pathlib import Path
from textwrap import dedent
from typing import NoReturn

from colors import green, red, yellow


def _log(message: str) -> None:
    print(message, file=sys.stderr)


def debug(message: str) -> None:
    logging.debug(message)


def info(message: str) -> None:
    logging.info(message)
    _log(green(message))


def warn(message: str) -> None:
    logging.warning(message)
    _log(yellow(message))


def fatal(message: str) -> NoReturn:
    logging.critical(message)
    sys.exit(red(message))


def exception(message: str, exc_info=None) -> NoReturn:
    logging.exception(message, exc_info=exc_info)
    sys.exit(red(message))


def init_logging(base_dir: Path, log_name: str):
    logging.root.setLevel(level=logging.DEBUG)

    log_file = base_dir / "logs" / f"{log_name}.log"
    log_file.parent.mkdir(parents=True, exist_ok=True)

    # This gets us ~5MB of logs max per version of scie-pants (since we're writing these under the
    # scie.bindings dir which is keyed to our lift manifest hash).
    debug_handler = RotatingFileHandler(filename=log_file, maxBytes=1_000_000, backupCount=4)
    debug_handler.setFormatter(
        logging.Formatter(fmt="{asctime} {levelname}] {name}: {message}", style="{")
    )
    logging.root.addHandler(debug_handler)

    sys.excepthook = lambda exc_type, exc, tb: exception(
        dedent(
            f"""\
            Install failed: {exc}
            More information can be found in the log at: {log_file}
            """
        ),
        exc_info=(exc_type, exc, tb),
    )
