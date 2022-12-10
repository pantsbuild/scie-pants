# Copyright 2020 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

# N.B.: We include the BUILD file itself to get a bytewise-identical PEX from the Pants package goal
# and the `cargo run -p package -- tools` command. The latter uses `pex -D tools/src` which picks up
# these BUILDs. Moving these targets to `tools/BUILD` would solve that problem but causes Pants
# issues since the targets contained within are then above the `tools/src` source root.
resources(
    sources=["**/*"]
)
