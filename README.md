# scie-pants

[![GitHub](https://img.shields.io/github/license/pantsbuild/scie-pants)](LICENSE)
[![Github Actions CI](https://github.com/pantsbuild/scie-pants/actions/workflows/ci.yml/badge.svg)](https://github.com/pantsbuild/scie-pants/actions/workflows/ci.yml)

The scie-pants binary is the next-generation `./pants` script.

It's currently in production use as the [recommended way to install Pants](
https://www.pantsbuild.org/stable/docs/getting-started/installing-pants) and is suitable for trying out in any existing
Pants-using project. We welcome reports of successes, failures and where things work but could be
better.

## Installing

See the official installation recommendations here for use as your `pants` launcher:
https://www.pantsbuild.org/stable/docs/getting-started/installing-pants

The binaries are released via [GitHub Releases](https://github.com/pantsbuild/scie-pants/releases)
for Linux (aarch64 and x86_64) and macOS (aarch64).

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
  are self-contained from the [Python Build Standalone](https://github.com/astral-sh/python-build-standalone) 
  project.

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

+ Partial support for firewalls

### Firewall support

Currently, you can re-direct the URLs used to fetch:

  + [Python Build Standalone](https://github.com/astral-sh/python-build-standalone/releases) CPython
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
    "pants.2.18.0-cp39-linux_x86_64.pex": "https://example.com/pants.2.18.0-cp39-linux_x86_64.pex",
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
PEX name (E.g. `pants.<version>-<python>-<plat>_<machine>.pex`). These can be found on the relevant
GitHub Release page's Assets (e.g. https://github.com/pantsbuild/pants/releases/tag/release_2.18.0a0).
(Note that for 2.18.x, PEX exist versioned and unversioned. `scie-pants` only uses the versioned
name as the key).

### Private Google Cloud Storage

Explicit `gs://bucket/object` entries in `PANTS_BOOTSTRAP_URLS` can be delegated to a
GCS-enabled native `ptex`, including Python downloads before any interpreter is installed:

```json
{
  "ptex": {
    "pants.2.33.1-cp314-linux_x86_64.pex": "gs://private-builds/releases/pants.pex"
  }
}
```

This requires **both** the launcher change and a bundled ptex containing GCS support.
The current `package/scie-pants.toml` pin (`1.7.0`) does not contain that support; change
it only once a GCS-enabled release is available, or package a local development build.
Installing a different ptex on `PATH` does not replace the launcher's embedded downloader.
Keep any Python mappings required by your launcher in the same map.

Configure Google Application Default Credentials before launching: set
`GOOGLE_APPLICATION_CREDENTIALS` to a trusted ADC configuration file, use existing local
ADC (for example, from `gcloud auth application-default login`), or use an attached
service account. The identity needs `storage.objects.get`. Credentials are resolved by
the native downloader, not passed as subprocess arguments or stored in the URL map.
For cold Python bootstrap, export credential settings in the parent environment rather
than relying on a project `.env` file being loaded first.

Explicit GCS mappings are authoritative: the launcher does not perform an unauthenticated
`urllib` HEAD request or fall back to a public release if the selected object cannot be
fetched. Authentication failures, missing objects, redirects and interrupted downloads
abort installation before executing the downloaded PEX. Existing `file:`, HTTP and HTTPS
mappings keep their current behavior; HTTPS GCS URLs do not automatically acquire ADC.

Use immutable object names and restrict bucket write access: private access is not an
artifact signature. Existing embedded Python hashes are still checked, but dynamically
selected Pants PEX mappings do not add a new integrity check. Query parameters (including
object generations) and requester-pays buckets are not supported by this implementation.

### Private GitHub artifacts and release assets

A GitHub-enabled native ptex accepts Actions browser artifact links, Actions API
archive links, and release-asset API links in `PANTS_BOOTSTRAP_URLS`. For example,
a raw release asset can supply the Pants PEX:

```json
{
  "ptex": {
    "pants.2.33.1-cp314-linux_x86_64.pex": "https://api.github.com/repos/example/builds/releases/assets/123"
  }
}
```

Keep all required Python mappings alongside this example: the override replaces the
embedded URL map, not just the Pants entry. The embedded downloader must contain the
new GitHub support; the current ptex `1.7.0` pin does not. Package a local development
build or use a future supporting release, rather than replacing ptex on `PATH`.

Export `PTEX_GITHUB_TOKEN` in the parent environment **before** cold bootstrap. Use a
repository-restricted fine-grained PAT/App token with **Contents: read** for release
assets or **Actions: read** for Actions artifacts. `GITHUB_TOKEN` from a workflow can
be explicitly assigned to this variable, but generally cannot read another private
repository. The downloader does not implicitly read `GH_TOKEN`, `GITHUB_TOKEN`,
GitHub CLI credentials, or the launcher's older GitHub API token setting. Tokens
never become ptex subprocess arguments or URL-map values; take care that a parent
launcher's environment may also be inherited by the application it starts.

Supported Actions links are:

- `https://github.com/OWNER/REPO/actions/runs/RUN_ID/artifacts/ARTIFACT_ID`
- `https://api.github.com/repos/OWNER/REPO/actions/artifacts/ARTIFACT_ID/zip`

**Default Actions artifacts are ZIP-wrapped and cannot replace a raw Pants PEX or
Python tarball.** ptex preserves bytes and does not extract archives. Use raw release
assets, or an `actions/upload-artifact` version supporting `archive: false` for a
single file. Embedded Python hashes must still match the original distribution.
Actions artifacts can expire with workflow retention, so release assets or GCS are
preferable for durable bootstrap sources.

The launcher delegates recognized explicit mappings directly to ptex, without an
unauthenticated HEAD request. Invalid links, missing credentials, expired artifacts
and failed/partial downloads abort installation before the PEX is executed; the
launcher does not fall back to a public release after selection. Native ptex confines
bearer authentication to the GitHub API and fetches its signed redirect with a fresh
credential-free request. Private release browser links (`/releases/download/...`)
and GitHub Enterprise are not handled by this provider; use the GitHub.com release
asset API URL. Ordinary public release links keep their existing behavior.

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

### Check pants installation logs

When the Python virtual environment (venv) for Pants is created, any logged output during that
process is saved in a `pants-install.log` file in the venv root. The path to the venv root is logged
during bootstrapping on a line with this pattern:

    New virtual environment successfully created at /.../nce/<hash value>/bindings/venvs/<pants version>


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
