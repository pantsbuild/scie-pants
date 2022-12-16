# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

import argparse
import json
import subprocess
from argparse import ArgumentParser, Namespace
from dataclasses import dataclass
from subprocess import CompletedProcess
from typing import Any, BinaryIO, Callable, cast


@dataclass(frozen=True)
class Ptex:
    @classmethod
    def from_exe(cls, exe: str) -> Ptex:
        return cls(exe)

    @classmethod
    def add_options(cls, parser: ArgumentParser) -> Callable[[Namespace], Ptex]:
        parser.add_argument(
            "--ptex-path",
            dest="ptex",
            required=True,
            type=cls.from_exe,
            # The path of a ptex binary.
            help=argparse.SUPPRESS,
        )
        return lambda options: cast(Ptex, options.ptex)

    _exe: str

    def _fetch(self, url: str, stdout: int, **headers: str) -> CompletedProcess:
        args = [self._exe]
        for header, value in headers.items():
            args.extend(("-H", f"{header}: {value}"))
        args.append(url)
        return subprocess.run(args=args, stdout=stdout, check=True)

    def fetch_json(self, url: str, **headers: str) -> dict[str, Any]:
        return json.loads(self._fetch(url, stdout=subprocess.PIPE, **headers).stdout)

    def fetch_text(self, url: str, **headers: str) -> str:
        return self._fetch(url, stdout=subprocess.PIPE, **headers).stdout.decode()

    def fetch_to_fp(self, url: str, fp: BinaryIO, **headers: str) -> None:
        self._fetch(url, stdout=fp.fileno(), **headers)
