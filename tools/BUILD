# Copyright 2020 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

poetry_requirements(
    name="reqs",
)

resource(
    name="pyproject",
    source="pyproject.toml"
)

python_distribution(
    name="wheel",
    provides=python_artifact(
        name="scie_pants",
    ),
    sdist=False,
    dependencies=[
        ":pyproject",
        "//tools/src/scie_pants",
    ],
    generate_setup = False,
)

pex_binary(
    name="tools",
    script="conscript",
    dependencies=[
        ":reqs#conscript",
        ":reqs#packaging",
        ":wheel",
    ]
)