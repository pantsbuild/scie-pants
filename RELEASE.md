# Release Process

## Preparation

### Version Bump and Changelog

1. Bump the version in [`Cargo.toml`](Cargo.toml).
2. Run `cargo run -p package -- test` to update [`Cargo.lock`](Cargo.lock) with the new version and
   as a sanity check on the state of the project.
3. Update [`CHANGES.md`](CHANGES.md) with any changes that are likely to be useful to consumers.
4. Open a PR with these changes and land it on https://github.com/pantsbuild/scie-pants main.

## Release

### Push Release Tag

Sync a local branch with https://github.com/pantsbuild/scie-pants main and confirm it has the
version bump and changelog update as the tip commit:

```
$ git log --stat -1
commit bb62b599ffb84189b2729cad77177f0146f364d9 (HEAD)
Author: John Sirois <john.sirois@gmail.com>
Date:   Sat Dec 17 10:48:31 2022 -0800

    Release 0.1.8.

    Upgrade to `scie-jump` 0.7.1 to avoid `SCIE_BOOT=update scie-pants`
    inifinite loop.

 CHANGES.md          | 2 +-
 Cargo.lock          | 2 +-
 Cargo.toml          | 2 +-
 package/src/main.rs | 2 +-
 4 files changed, 4 insertions(+), 4 deletions(-)
```

Tag the release as `v<version>` and push the tag to https://github.com/pantsbuild/scie-pants main:

```
$ git tag --sign -am 'Release 0.1.9' v0.1.9
$ git push --tags https://github.com/pantsbuild/scie-pants HEAD:main
```

The release is automated and will create a GitHub Release page at
[https://github.com/pantsbuild/scie-pants/releases/tag/v&lt;version&gt;](
https://github.com/pantsbuild/scie-pants) with binaries for Linux & Mac.

