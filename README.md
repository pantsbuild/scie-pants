# scie-pants

[![GitHub](https://img.shields.io/github/license/pantsbuild/scie-pants)](LICENSE)
[![Github Actions CI (x86_64 Linux / MacOS / Windows)](https://github.com/pantsbuild/scie-pants/actions/workflows/ci.yml/badge.svg)](https://github.com/pantsbuild/scie-pants/actions/workflows/ci.yml)
[![CircleCI (Linux aarch64)](https://circleci.com/gh/pantsbuild/scie-pants.svg?style=svg)](https://circleci.com/gh/pantsbuild/scie-pants)

The scie-pants binary is the next-generation `./pants` script.

It's currently in production use as the [recommended way to install Pants](
https://www.pantsbuild.org/docs/installation) and is suitable for trying out in any existing
Pants-using project. We welcome reports of successes, failures and where things work but could be
better.

## Installing

See the official installation recommendations here for use as your `pants` launcher:
https://www.pantsbuild.org/docs/installation

The binaries are released via [GitHub Releases](https://github.com/pantsbuild/scie-pants/releases)
for both Linux and macOS and both aarch64 and x86_64 chip architectures.

If you'd like to build your own version, see the [contribution guide](CONTRIBUTING.md). There are
build instructions there.

## Features

You can run `scie-pants` anywhere you'd use `./pants` and other places besides. It should work just
like your existing `./pants` script but with ~33% less latency launching Pants.

Beyond support for all known existing `./pants` script features, the `scie-pants` executable
provides the following:

+ A hermetic Python interpreter appropriate for the current active Pants version:

  You no longer need to have Python of the right version, or even any Python at all, installed on
  your system to use Pants. If you use `scie-pants` in a repo with old Pants (< 2.5.0), it will
  fetch a CPython 3.8 interpreter to use and a CPython 3.9 interpreter otherwise. These interpreters
  are self-contained from the [Python Build Standalone](
  https://python-build-standalone.readthedocs.io/en/latest/) project.

+ Support for `.env` files:

  The first `.env` file found in the current directory or any of its parent directories is loaded
  and exported into Pants (and scie-pants) environment.

+ The ability to run Pants in a subdirectory of your project:

  This is of limited utility since Pants internals don't support this well at the moment, but as
  soon as they do, `scie-pants` will allow you to work in the style you prefer.

+ Built-in ability to set up a new Pants project:

  If you run `scie-pants` in a directory where Pants is not already set up, it will prompt you, and
  you can let it set up the latest Pants stable version for your project.

+ Built-in [`pants_from_sources`](
  https://github.com/pantsbuild/example-python/blob/1b38d08821865e3756024950bc000bdbd0161b95/pants_from_sources)
  support. You can either execute `scie-pants` with `PANTS_SOURCE` set to the path of a local clone
  of the [Pants](https://github.com/pantsbuild/pants) repo or else copy, link or symlink your
  `scie-pants` executable to `pants_from_sources` and execute that. In this case `PANTS_SOURCE` will
  default to `../pants` just as was the case in the bespoke `./pants_from_sources` scripts.

+ Partial support for firewalls:

  Currently, you can re-direct the URLs used to fetch:

    + [Python Build Standalone](https://python-build-standalone.readthedocs.io/en/latest/) CPython
      distributions used to bootstrap Pants.
    + Pants PEX release assets which contain Pants as a single-file application.

  This is done by exporting a `PANTS_BOOTSTRAP_URLS` environment variable
  specifying the path to a JSON file containing a mapping of file names to URLS to fetch them from
  under a top-level `"ptex"` key. For example:
  ```json
  {
    "ptex": {
      "cpython-3.8.16+20230507-x86_64-unknown-linux-gnu-install_only.tar.gz": "https://example.com/cpython-3.8.16%2B20230507-x86_64-unknown-linux-gnu-install_only.tar.gz",
      "cpython-3.8.16+20230507-aarch64-apple-darwin-install_only.tar.gz": "https://example.com/cpython-3.8.16%2B20230507-aarch64-apple-darwin-install_only.tar.gz",
      "cpython-3.9.16+20230507-x86_64-unknown-linux-gnu-install_only.tar.gz": "https://example.com/cpython-3.9.16%2B20230507-x86_64-unknown-linux-gnu-install_only.tar.gz",
      "cpython-3.9.16+20230507-aarch64-apple-darwin-install_only.tar.gz": "https://example.com/cpython-3.9.16%2B20230507-aarch64-apple-darwin-install_only.tar.gz",
      "pants.2.18.0-cp9-linux-x86_64.pex": "https://example.com/pants.2.18.0-cp9-linux-x86_64.pex",
      ...
    }
  }
  ```

  For keys that are "embedded" into `scie-pants` itself (such as Python Build Standalone), you can run:
  ```
  $ SCIE=inspect scie-pants | jq .ptex
  ```
  You'll need to run this once for each platform you use `scie-pants` on to gather all mappings
  you'll need; e.g.: once for Linux x86_64 and once for Mac ARM.

  The embedded artifact references also contain expected hashes of the downloaded content. Your
  re-directed URLs must provide the same content as the canonical URLs; if the hashes of downloaded
  files do not match those recorded in `scie-pants`, install will fail fast and let you know about
  the hash mismatch.

  For other keys that aren't embedded, and are generated on-the-fly (such as the Pants PEX), there
  is no single source of truth that can be easily scraped out. For the Pants PEX, the key is the versioned
  PEX name (E.g. `pants.<version>-<python>-<plat>-<machine>.pex`). These can be found on the relevant
  GitHub Release page's Assets (e.g. https://github.com/pantsbuild/pants/releases/tag/release_2.18.0a0).
  (Note that for 2.18.x, PEX exist versioned and unversioned. `scie-pants` only uses the versioned
  name as the key).

## Caveats

The `scie-pants` binary will re-install versions of Pants you have already installed. The underlying
[`scie`](https://github.com/a-scie/jump/blob/main/README.md) technology uses an `nce` cache
directory that is different from the `~/.cache/pants/setup` directory used by the `./pants` script.
This is a one-time event per Pants version.

## Solving Problems

### Try upgrading

If you run into an issue with `scie-pants` you might 1st try upgrading. You can do this with:
```
SCIE_BOOT=update scie-pants
```

That will update to the latest available stable release if there is a newer one and tell you if
there is not. You can also supply a `scie-pants` version as the sole argument to downgrade or switch
to a specific version.

### Report an issue

You can report an issue directly at https://github.com/pantsbuild/scie-pants/issues. Please include
the `scie-pants` version you're using. You can get this by running:
```
PANTS_BOOTSTRAP_VERSION=report scie-pants
```

You might want to check the existing issues first though. There are some known features and bugs on
the roadmap you may have run into and if there is an existing issue, you can chime in on your
support for it or your particular take on it.

### Chip in

There is a [contribution guide](CONTRIBUTING.md) and more developer docs are coming soon. Any help
fixing bugs or improving UX is very welcome.
