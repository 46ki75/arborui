# Git Repository Standards

## Contents

- [EditorConfig](#create-editorconfig)
- [Package manager](#package-manager-pnpm)
- [Git hooks](#git-hooks-lefthook)
- [Markdown linting](#markdown-linting-with-markdownlint-cli2)

## Create `.editorconfig`

```ini
# editorconfig.org
root = true

[*]
end_of_line = lf
charset = utf-8
trim_trailing_whitespace = true
insert_final_newline = true
```

## Package manager: pnpm

Use pnpm for anything Node-based — including repos whose primary
language is not JavaScript but which carry Node tooling (e.g.
`markdownlint-cli2` in a Rust repo). This is org policy; do not switch
to npm or yarn because a repo happens to have their artifacts lying
around.

- Declare `"packageManager": "pnpm@<exact-version>"` in `package.json`.
  Run `corepack use pnpm@<version>` rather than typing the version by hand.
- Commit `pnpm-lock.yaml`; never `package-lock.json` or `yarn.lock`.
- CI installs with `pnpm install --frozen-lockfile`.

Exception: Bun projects (see `references/bun/`), where Bun is both
runtime and package manager.

## Git hooks: lefthook

Use [lefthook](https://github.com/evilmartians/lefthook) for git hooks, not
husky. Declare it as a root `devDependency` and install it via the
standard npm lifecycle hook:

```json
{
  "scripts": {
    "prepare": "lefthook install"
  },
  "devDependencies": {
    "lefthook": "^2.1.9"
  }
}
```

The root `prepare` script installs the hook explicitly. Do not also allow a
dependency lifecycle script for lefthook; that duplicates installation and
grants unnecessary install-time execution.

### Single-package repos

A flat `pre-commit` group covering every language present is enough:

```yaml
pre-commit:
  jobs:
    - name: rustfmt
      glob: "**/*.rs"
      run: cargo fmt -- {staged_files}
      stage_fixed: true
    - name: markdownlint
      glob: "*.md"
      run: pnpm exec markdownlint-cli2 --fix {staged_files}
    - name: terraform-fmt
      glob: "**/*.tf"
      run: terraform fmt {staged_files}
```

### pnpm monorepos

Scope each job to its package with `root:`/`glob:` so a tool never has to
guess which package's config applies:

```yaml
pre-commit:
  jobs:
    - run: pnpm run --recursive check

check:
  jobs:
    - name: eslint-foo
      root: "packages/foo/"
      glob: "packages/foo/**/*.{ts,tsx}"
      run: pnpm --filter foo lint
    - name: vitest-foo
      root: "packages/foo/"
      glob: "packages/foo/**/*.{ts,tsx}"
      run: pnpm --filter foo exec vitest related --run --passWithNoTests {files}
```

Tools with no meaningful per-package config (e.g. Prettier across a whole
monorepo) run once from the repo root instead of being duplicated per
package.

### File-list template convention

Pick the template by where the file list actually comes from:

- **Real Git hooks** use the template that names their source:
  `{staged_files}` in `pre-commit`, `{push_files}` in `pre-push`. The
  template name documents the data source.
- **Custom manual-run groups** (`check`, `fmt`, `claude-check`, … —
  anything invoked via `lefthook run <group> --file <path>` or
  `--all-files`) use `{files}`. The caller supplies the list; the
  `--file`/`--all-files` flags force-substitute it into whatever template
  the job uses, so `{staged_files}` would *work* there, but it claims a
  data source that isn't real. `{files}` also has the safer fallback: with
  no `files:` command and no `--file` flag, the list is empty and every
  job skips, whereas a bare `lefthook run` of a `{staged_files}` group
  would run against whatever happens to be staged at that moment.

### Claude Code hook integration

Wire Claude Code to the same checks as `pre-commit` so the agent fixes
lint errors at edit time instead of at commit time. Three pieces:

**1. A custom `claude-check` group in `lefthook.yml`** — same jobs as
`pre-commit`, with two deliberate differences: `{files}` instead of
`{staged_files}` (see the template convention above), and **no
`stage_fixed`** — the agent's edits must never be staged implicitly.

```yaml
# Custom hook (not a Git hook): run by the Claude Code PostToolUse hook on
# each file the agent edits — `lefthook run claude-check --file <path>`.
claude-check:
  parallel: true
  jobs:
    - name: ruff-format
      glob: "*.py"
      run: uv run ruff format {files}
    - name: ruff-check
      glob: "*.py"
      run: uv run ruff check --fix {files}
    - name: markdownlint
      glob: "*.md"
      run: pnpm exec markdownlint-cli2 {files}
```

**2. `.claude/settings.json`** — a `PostToolUse` hook on `Edit|Write`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/post-edit.sh"
          }
        ]
      }
    ]
  }
}
```

**3. `.claude/hooks/post-edit.sh`** (committed, `chmod +x`):

```sh
#!/bin/sh
# Claude Code PostToolUse hook: run the lefthook `claude-check` jobs on the
# file the agent just edited. Exit 2 feeds the tool output back to the agent
# so it can fix lint errors itself.
set -u

file=$(jq -r '.tool_input.file_path // empty')
[ -z "$file" ] && exit 0

# Only check files inside this project.
case "$file" in
  "$CLAUDE_PROJECT_DIR"/*) ;;
  *) exit 0 ;;
esac

cd "$CLAUDE_PROJECT_DIR" || exit 0

out=$(NO_COLOR=1 pnpm exec lefthook run claude-check --file "$file" 2>&1) || {
  echo "$out" >&2
  exit 2
}
exit 0
```

Behavioral notes, each load-bearing:

- **Exit 2 on failure, output on stderr** — that is the PostToolUse
  contract for feeding diagnostics back to the agent; any other non-zero
  exit only shows the user.
- **Auto-fixable issues exit 0 silently** — formatters and `--fix` rules
  repair the file in place; only unfixable diagnostics (undefined names,
  real lint errors) block and round-trip to the agent.
- **`NO_COLOR=1`** — lefthook's decorated output is ANSI-heavy; without
  it the agent receives escape-code soup.
- **Skip files outside `$CLAUDE_PROJECT_DIR`** — the agent also writes to
  scratchpads and other working directories; linting those is noise.
- Hooks are snapshotted at session start: a newly added
  `.claude/settings.json` takes effect in new sessions (or after `/hooks`
  review in the current one).

## Markdown linting with `markdownlint-cli2`

Every repository that contains Markdown should lint it with
[`markdownlint-cli2`](https://github.com/DavidAnson/markdownlint-cli2).
Use the config and `package.json` shape below — do not improvise rule
sets per repo.

### `.markdownlint-cli2.yaml`

Place at the repository root. **YAML, not JSONC** — `.markdownlint-cli2.jsonc`
with only an `ignores` array (no `MD013`/`MD024`/`MD029` block) is a drift
pattern seen in some org repos, not an accepted alternative. A bare-defaults
config means 80-character line wrapping and default list/heading rules,
which is not what this org has decided on. If you find a `.jsonc` config
with no rule overrides in a repo, that repo is out of compliance — bring it
to `.yaml` with the block below rather than treating the `.jsonc` file as
precedent.

```yaml
config:
  MD013:
    line_length: 200
    tables: false
    code_blocks: false
  MD024:
    siblings_only: true
  MD029:
    style: ordered
ignores:
  - "**/node_modules/**"
```

Rule choices, and why:

- **MD013 (line length) → 200, tables/code blocks exempt.** Long URLs,
  command examples, and tables routinely exceed the default 80. A hard
  wrap there hurts readability more than it helps. 200 is generous
  enough that real prose still gets flagged.
- **MD024 (no duplicate headings) → `siblings_only: true`.** Repeated
  headings under different parents (e.g. `## Install` appearing under
  multiple OS sections) are legitimate. Only flag duplicates at the
  same level.
- **MD029 (ordered list prefix) → `ordered`.** Use `1.`, `2.`, `3.`
  literally — never the `1.`, `1.`, `1.` lazy style. The rendered
  output is the same, but the source stays diff-friendly and human-
  readable.

### `ignores`

Add globs for anything the linter should not touch. Common entries:

| Path                  | Reason                                                                      |
| --------------------- | --------------------------------------------------------------------------- |
| `**/node_modules/**`  | Third-party packages.                                                       |
| `./submodules/**`     | Git submodules — upstream owns their lint rules.                            |
| `./target/**`         | Rust build output.                                                          |
| `./.claude/**`        | Agent-generated transcripts and worktrees.                                  |
| `./refs/`, `./notes/` | Local-only scratch / reference material, if the repo uses such conventions. |

Vendored or generated Markdown (third-party docs copied in, generated
API docs) should also be ignored — fix it upstream, not here.

### `package.json`

Pin `markdownlint-cli2` as a dev dependency and expose a `lint` script:

```json
{
  "packageManager": "pnpm@11.13.0",
  "scripts": {
    "lint": "markdownlint-cli2 \"**/*.md\""
  },
  "devDependencies": {
    "markdownlint-cli2": "^0.22.1"
  }
}
```

(Pin `packageManager` to the current approved pnpm release — the field requires
an exact version.)

The glob in the script is the lint **target**; `ignores` in the YAML
is the exclusion list. Keep both — narrowing the glob to skip a
directory hides the file from `--fix` runs too.

### Running

```bash
pnpm lint              # or, in Bun projects: bun run lint
pnpm exec markdownlint-cli2 --fix "**/*.md"   # auto-fix where possible
```

Wire `pnpm lint` into CI (after `pnpm install --frozen-lockfile`) so
Markdown regressions fail the build the same way code-lint regressions
do. This step is commonly skipped in practice — several org repos enforce
markdown lint only through the editor extension below, with no CI job at
all, which means a contributor who ignores editor diagnostics can merge
unlinted Markdown. Don't copy that gap into a new repo.

### Editor integration

Recommend the VS Code extension
[`DavidAnson.vscode-markdownlint`](https://marketplace.visualstudio.com/items?itemName=DavidAnson.vscode-markdownlint).
It reads the same `.markdownlint-cli2.yaml`, so editor diagnostics
match CI output exactly. Add it to `.vscode/extensions.json` under
`recommendations` so contributors get prompted on first open:

```json
{
  "recommendations": ["DavidAnson.vscode-markdownlint"]
}
```
