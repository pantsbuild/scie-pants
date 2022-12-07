# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import os
import subprocess
import sys
from subprocess import CompletedProcess


def run_tool(tool: str, *args: str, **kwargs) -> CompletedProcess:
    tools_pex = os.environ.get("TOOLS_PEX")
    assert tools_pex is not None, (
        "The tools.pex must be built and its path communicated by the TOOLS_PEX environment "
        "variable."
    )
    return subprocess.run(args=[sys.executable, tools_pex, tool, *args], **kwargs, check=True)
