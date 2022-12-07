# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import os
import sys

import pytest


def pytest_sessionstart(session: pytest.Session) -> None:
    sys.path.append(os.path.join(session.config.rootpath, "test_support"))
