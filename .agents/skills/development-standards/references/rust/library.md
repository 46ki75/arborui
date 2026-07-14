# Rust Library Standards

Conventions specific to a **publishable** Rust crate (crates.io or an
internal registry), layered on top of
[`references/rust/general.md`](general.md) — workspace inheritance,
`rust-toolchain.toml`, `just`, `cargo-llvm-cov`, and the async-trait
guidance there all still apply. This file only covers what changes once a
crate has external consumers.

The org's reference example is a published, multi-crate SDK
(`46ki75/notionrs`). Where it demonstrably does **not** do something a
"hardened published crate" checklist would suggest, that's called out
explicitly below as a gap to consider closing, not silently adopted as the
convention.

## Workspace layout for a multi-crate SDK

`general.md`'s `members = ["crates/*"]` glob assumes a `crates/` wrapper
directory. An equally valid alternative for a published SDK is sibling
top-level directories named after each crate, with no wrapper:

```toml
[workspace]
resolver = "3"
members = ["foo", "foo_macro", "foo_types", "foo_webhooks"]
```

```text
foo/            # public API client
foo_macro/      # proc-macro crate, internal-only
foo_types/      # shared schema/struct definitions
foo_webhooks/   # independent leaf crate
```

Pick whichever layout matches how the crates are actually named on
crates.io — a `crates/*` glob reads oddly once package names no longer
share a directory prefix with their path.

### Split out a proc-macro crate

A crate that needs a derive/attribute macro puts it in its own workspace
member with `proc-macro = true`, consumed internally by the crates that
need it:

```toml
# foo_macro/Cargo.toml
[lib]
proc-macro = true
```

It's fine for this crate to be published (Cargo requires a proc-macro
crate to be its own compilation unit, which for public dependents means
its own published crate) while being **documented as internal-only** —
omit `readme`, `categories`, and `keywords`, and say so in its README:
publishing to crates.io and being a supported public API are different
decisions.

### Crates.io metadata on `[workspace.package]`

In addition to what `general.md` already inherits (`edition`,
`rust-version`, `license`, `authors`, `repository`), a published-crate
workspace also centralizes crates.io listing metadata:

```toml
[workspace.package]
categories = ["web-programming::http-client"]
keywords = ["foo", "foo-api"]
```

Members inherit both with `categories.workspace = true` /
`keywords.workspace = true`, same as any other workspace-level field. A
member with genuinely different keywords/categories (rare) sets its own
instead of inheriting — don't force a leaf crate's listing to match the
main crate's if it doesn't.

## Independent versioning inside one workspace

When workspace members version independently (the macro/types/webhooks
crates don't necessarily bump on the same cadence as the main crate), do not
inherit `version` from `[workspace.package]`. Give each member its own
`package.version`; the rest of the workspace inheritance rules still apply.
Tag the main crate bare and prefix every other crate's tag with its name:

```text
v0.27.0            # main crate release
macro-v0.4.0        # foo_macro release
webhooks-v0.2.0     # foo_webhooks release
```

## Release notes without a `CHANGELOG.md`

An alternative to an in-repo `CHANGELOG.md` is a standalone
`notes/vX.Y.Z.md` file per release, drafted from a checked-in prompt
template rather than hand-written from scratch:

```text
.github/prompts/create-release-note-draft.prompt.md   # authoring spec: audience,
                                                        # PR-label categorization
                                                        # (Conventional Commits fallback),
                                                        # dedup rules, omit-empty-sections
notes/v0.27.0.md                                       # the drafted note for this release
```

Because the workspace has multiple independently-versioned crates, close
the note with a per-crate version table so a reader can tell at a glance
which crate versions ship together:

```markdown
**Crate versions in this release:**

| Crate    | Version |
| -------- | ------- |
| foo      | 0.27.0  |
| foo_types| 0.20.0  |
| foo_macro| 0.4.0   |
```

## Release process: manual, checklist-gated, tag-protected

Don't build a `publish-on-tag` CI workflow by default — the observed
pattern is a manual release, walked through via a PR template checklist:

```markdown
<!-- .github/PULL_REQUEST_TEMPLATE/release.md -->

- [ ] Confirm that the PR target is the `main` branch
- [ ] Verify that all release contents are included in the `release` branch
- [ ] Confirm that integration tests have passed
- [ ] Update version information (e.g., in `Cargo.toml`)
- [ ] Confirm that the crate has been published
- [ ] Confirm that the release tag has been created and pushed
- [ ] Confirm that release notes have been created
```

The actual enforcement isn't CI — it's a Terraform-managed GitHub ruleset
that locks both main-crate `refs/tags/v*` and independently versioned
`refs/tags/*-v*` releases against creation, update, or deletion by anyone
but an admin (see `references/terraform/general.md` § _GitHub repository
administration as Terraform_). That combination — human-run checklist for
the happy path, ruleset for the thing that would actually cause damage if
gotten wrong — is the deliberate design, not a missing automation step.
If you do want tag-triggered `cargo publish` automation later, add it
alongside this checklist rather than instead of the ruleset.

## Testing: gate by "safe for an agent to run," not just "hits the network"

`general.md`'s hermetic-vs-live split assumes "live" only costs money or
needs secrets. A library whose integration tests exercise a real
third-party API that can also be **mutated** (not just read) needs a
sharper cut: split by `readonly` vs. `mutable` instead, as two separate
test binaries. Keep mutable tests `#[ignore]`d so an ordinary `cargo test`
cannot run them:

```text
tests/
  integration_test_readonly.rs   # mod readonly; safe to run unattended
  integration_test_mutable.rs    # mod mutable; NOT safe to run unattended
  readonly/
  mutable/
```

```rust
#[tokio::test]
#[ignore = "mutable: changes the real FOO account; run only with human approval"]
async fn search() -> Result<(), foo::Error> {
    dotenvy::dotenv().ok();
    let api_key = std::env::var("FOO_API_KEY_MUTABLE").unwrap();
    let client = foo::Client::new(api_key);
    // ...
}
```

State the rule explicitly in the crate's contributor docs, not just in
CI config: **mutable integration tests must not be run by an AI agent**
unsupervised. CI runs the readonly tier automatically. A human runs the
mutable tier explicitly before a release with:

```bash
cargo test --test integration_test_mutable -- --ignored
```

## Dual agent docs: `AGENTS.md` real, `CLAUDE.md` a symlink to the README

One org repo uses a pattern worth naming even though it's currently a
single data point — propose it rather than treat it as settled:

- `AGENTS.md` is a real file: contributor/agent-facing instructions
  (directory map, MSRV, git tag rules, coverage command, test-tier
  safety rules). Written for whoever is *working on* the crate.
- `CLAUDE.md` is a **symlink to `README.md`** — so Claude Code's
  auto-loaded project memory is exactly the crate's public-facing README
  (badges, feature list, usage example), not a duplicate operating manual.
  The README carries an explicit cross-reference:

  ```markdown
  > [!NOTE]
  > `AGENTS.md` is written for AI agents and internal contributors, not for crate users.
  > If you're consuming this crate, see the API Reference above.
  ```

This avoids maintaining two descriptions of "what is this crate" (one in
`README.md`, a drifted second one in `CLAUDE.md`) while still giving
coding agents a separate, contributor-focused document to work from.

## Gaps worth closing, not conventions to copy

The org's reference SDK does **not** currently do the following. Treat
each as a forward-looking recommendation if you're hardening a new
published crate — not as "what we do here," since no audited repo actually
does it yet:

- **`[package.metadata.docs.rs]`** — absent everywhere. A crate with
  mutually exclusive feature flags (e.g. multiple TLS backends) needs this
  to render its full API on docs.rs:

  ```toml
  [package.metadata.docs.rs]
  all-features = true
  rustdoc-args = ["--cfg", "docsrs"]
  ```

- **Feature-flag documentation.** Feature flags exist in `Cargo.toml` with
  no corresponding doc-comment, README section, or module doc explaining
  what each one does or why you'd pick it. Document every public feature
  flag somewhere a consumer will actually see it — the crate's top-level
  `//!` doc comment or its README, not just the `[features]` table.
- **Sealed traits / `#[doc(hidden)]`** for forward-compatibility on public
  traits — not used anywhere. If a trait is meant to be implemented only
  by types inside the crate (so new required methods aren't a breaking
  change for consumers), seal it deliberately rather than leaving every
  public trait open to external `impl`.
- **`cargo-public-api` or `cargo-semver-checks`** in CI — absent. For a
  crate with real external consumers, a semver-diffing check in CI catches
  accidental breaking changes before they ship; right now nothing in the
  org does this for any published crate.
