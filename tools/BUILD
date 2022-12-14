# Copyright 2020 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

python_requirements(
    name="reqs",
)

python_requirements(
    name="test-reqs",
    source="test-requirements.txt",
)

pex_binary(
    name="tools",
    script="conscript",
    dependencies=[
        ":reqs#conscript",
        "//tools/src/scie_pants",
        "//tools/src/scie_pants.dist-info",
    ],
    execution_mode="venv",
    emit_warnings=False,
)
