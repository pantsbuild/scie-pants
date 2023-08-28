# Copyright 2020 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

python_test_utils(
    name="test_utils",
)

python_tests(
    runtime_package_dependencies=["//tools"],
    extra_env_vars=[
        "COLUMNS",
        "EXPECTED_COLUMNS",
        "EXPECTED_LINES",
        "GIT_COMMIT",
        "LINES",
        "PANTS_DOCKER_TOOLS",
        "TERM",
        "TOOLS_PEX=tools/tools.pex",
    ],
)
