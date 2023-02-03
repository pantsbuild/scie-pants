# scie-pants

[![GitHub](https://img.shields.io/github/license/pantsbuild/scie-pants)](LICENSE)
[![Github Actions CI (x86_64 Linux / MacOS / Windows)](https://github.com/pantsbuild/scie-pants/actions/workflows/ci.yml/badge.svg)](https://github.com/pantsbuild/scie-pants/actions/workflows/ci.yml)
[![CircleCI (Linux aarch64)](https://circleci.com/gh/pantsbuild/scie-pants.svg?style=svg)](https://circleci.com/gh/pantsbuild/scie-pants)

The scie-pants binary is intended to be the next-generation `./pants` script.

It's currently in a phase of development suitable for trying out in any existing Pants-using
project. We welcome early adopters who want to try it out and report successes, failures and where
things work but could be better.

## Installing

For now, you'll need to download the correct binary for your system, mark it as executable and place
it on your $PATH somewhere.

The binaries are released via [GitHub Releases](https://github.com/pantsbuild/scie-pants/releases)
for both Linux and macOS and both aarch64 and x86_64 chip architectures. Pants itself does not
support Linux aarch64 quite yet, but `scie-pants` is ready for it!

I run on Linux x86_64; so I install a stable release like so:
```
curl -fLO \
  https://github.com/pantsbuild/scie-pants/releases/download/v0.1.9/scie-pants-linux-x86_64
curl -fL \
  https://github.com/pantsbuild/scie-pants/releases/download/v0.1.9/scie-pants-linux-x86_64.sha256 \
  | sha256sum -c -
chmod +x scie-pants-linux-x86_64 && mv scie-pants-linux-x86_64 ~/bin/scie-pants
```

You can then run `scie-pants` anywhere you'd use `./pants` and other places besides. It should work
just like your existing `./pants` script but with ~33% less latency launching Pants.

If you'd like to build you own version, see the [contribution guide](CONTRIBUTING.md). There are
build instructions there.

## Features

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

  Currently, you can only re-direct the URLs scie-pants uses to fetch [Python Build Standalone](
  https://python-build-standalone.readthedocs.io/en/latest/) CPython distributions it uses to
  bootstrap Pants with. This is done by exporting a `PANTS_BOOTSTRAP_URLS` environment variable
  specifying the path to a JSON file containing a mapping of file names to URLS to fetch them from
  under a top-level `"ptex"` key. For example:
  ```json
  {
    "ptex": {
      "cpython-3.8.15+20221106-aarch64-unknown-linux-gnu-install_only.tar.gz": "https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.8.15+20221106-aarch64-unknown-linux-gnu-install_only.tar.gz",
      "cpython-3.8.15+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz": "https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.8.15+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz",
      "cpython-3.8.15+20221106-aarch64-apple-darwin-install_only.tar.gz": "https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.8.15+20221106-aarch64-apple-darwin-install_only.tar.gz",
      "cpython-3.8.15+20221106-x86_64-apple-darwin-install_only.tar.gz": "https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.8.15+20221106-x86_64-apple-darwin-install_only.tar.gz",
      "cpython-3.9.15+20221106-aarch64-unknown-linux-gnu-install_only.tar.gz": "https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.9.15+20221106-aarch64-unknown-linux-gnu-install_only.tar.gz",
      "cpython-3.9.15+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz": "https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.9.15+20221106-x86_64-unknown-linux-gnu-install_only.tar.gz",
      "cpython-3.9.15+20221106-aarch64-apple-darwin-install_only.tar.gz": "https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.9.15+20221106-aarch64-apple-darwin-install_only.tar.gz",
      "cpython-3.9.15+20221106-x86_64-apple-darwin-install_only.tar.gz": "https://github.com/indygreg/python-build-standalone/releases/download/20221106/cpython-3.9.15+20221106-x86_64-apple-darwin-install_only.tar.gz"
    }
  }
  ```
  To see the current mapping used by your version of `scie-pants` you can run:
  ```
  $ SCIE=inspect scie-pants | jq .ptex
  ```
  The keys in your re-mapping must match, but the URLs, of course will be different; presumably from
  a private network server or file share. You can omit keys for files you know you won't use. For
  example, for these CPython distributions, you can omit The 2 Linux aarch64 entries if you have no
  such machines. The full output of the inspect command can be used to examine the expected file
  size and hash of each of these. Your re-directed URLs must provide the same content; if the hashes
  of downloaded files do not match those recorded in scie-pants, install will fail fast and let you
  know about the hash mismatch. Once Pants itself starts shipping scies, those will also be able to
  redirected using the same file.

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
