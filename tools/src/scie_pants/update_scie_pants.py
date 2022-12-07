# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

# TODO(John Sirois): XXX
# For update:
# { "scie-pants": { "releases-url": "https://api.github.com/repos/a-scie/jump/releases" } }
# curl -fsSL https://api.github.com/repos/a-scie/jump/releases | jq '.[] | {version: .tag_name, assets: [.assets[] | {name: .name, url: .browser_download_url}]}'
# [{"tag_name", "assets": [ {"name", "browser_download_url"} ]}]
