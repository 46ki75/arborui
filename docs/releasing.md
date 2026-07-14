# Releasing

Releases are manual, coordinated, and checklist-gated. All publishable crates
use the same version and one `vX.Y.Z` tag.

## First-Release Name Gate

The eleven `arborui` package names had no exact crates.io matches when the
project was renamed. Recheck the complete package family immediately before the
first release because availability can change. The release script requires
`ARBORUI_CRATES_IO_NAME_CONFIRMED=1` for a real upload so this check cannot be
bypassed accidentally.

## Checklist

- Confirm the release is based on `main` and the worktree is clean.
- Update the coordinated workspace version and internal exact dependency
  versions.
- Add release and compatibility notes, including any MSRV change.
- Run `just ci`, `just test-pty`, `just package-check`, and
  `just publish-dry-run`.
- Confirm scheduled fuzzing and benchmark runs are healthy.
- Tag the verified commit as `vX.Y.Z`.
- Run the `Release` workflow against that tag with `publish` enabled.
- Verify all package versions on crates.io before creating the GitHub release.

## Toolchains

Project verification uses the Rust 1.85.0 MSRV. Publishing uses Cargo 1.90.0
because stable multi-package publication can package and verify interdependent
workspace crates in one operation. Publication is not atomic; if an upload
fails, inspect crates.io before retrying because already-uploaded versions are
immutable.

## SemVer Checks

After the first coordinated release exists, compare every package against the
latest `vX.Y.Z` tag with `cargo-semver-checks`. Findings block patch releases;
intentional breaking changes require a minor version increment and release-note
entry.
