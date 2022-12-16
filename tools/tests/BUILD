# Copyright 2020 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

python_test_utils(
    name="test_utils",
)

python_tests(
    runtime_package_dependencies=[
        "//tools"
    ],
    extra_env_vars=[
        "GIT_COMMIT",
        "TOOLS_PEX=tools/tools.pex",
    ],
)
