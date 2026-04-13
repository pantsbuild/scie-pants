# Release Process

## Preparation

1. Bump the version in [`Cargo.toml`](Cargo.toml).
2. Run `cargo run -p package -- test` to update [`Cargo.lock`](Cargo.lock) with the new version and as a sanity check on the state of the project.
3. Update [`CHANGES.md`](CHANGES.md) with any changes that are likely to be useful to consumers.
4. Open a PR with these changes and land it on https://github.com/pantsbuild/scie-pants main.

## Release

The release process is driven by GitHub Actions workflows.

On success, a new release is published to https://github.com/pantsbuild/scie-pants/releases with Linux and Mac binaries. Additionally, a new discussion announcement will be opened in https://github.com/pantsbuild/scie-pants/discussions, a Slack notification will be sent in `#announce`, and the [HomeBrew tap](https://github.com/pantsbuild/homebrew-tap) will get a version bump. 

Once the version/CHANGES.md update (in `Preparation` above) has landed, kick off a GitHub release via the [Actions Web UI](https://github.com/pantsbuild/scie-pants/actions/workflows/release.yml), or using the `gh` cli (where `{{ A.B.C }}` is the version of interest - e.g. `0.13.2`):

```bash
gh workflow run "release.yml" \
   --raw-field "tag=v{{ A.B.C }}" \
   --repo pantsbuild/scie-pants

# ✓ Created workflow_dispatch event for release.yml at main
# https://github.com/pantsbuild/scie-pants/actions/runs/XYZ

# To see the created workflow run, try: gh run view XYZ
# To see runs for this workflow, try: gh run list --workflow="release.yml"
```

Alternatively, a pre-release can be kicked off via:

```bash
gh workflow run "release.yml" \
   --raw-field "tag=v0.13.2-beta.1" \
   --raw-field "prerelease=true" \
   --repo pantsbuild/scie-pants 
```

The only substantial difference in declaring a pre-release is that the `tag` field skips versioning checks (i.e. any name is fine) and there will be no Slack/HomeBrew announcements or updates for pre-release builds.
