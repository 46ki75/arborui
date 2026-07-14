# Terraform Generic Development Standards

Org-wide convention for any repo that manages infrastructure or platform
config (AWS resources, GitHub repo settings) with Terraform.

## Layout: flat, one file per resource group, no `modules/`

Every audited Terraform config is flat — one `.tf` file per resource group
(`lambda.tf`, `dynamodb.tf`, `s3.tf`, `provider.tf`, ...) directly under
`terraform/`, with no `modules/` directory:

```text
terraform/
  provider.tf     # backend, required_providers, provider blocks
  lambda.tf
  dynamodb.tf
  s3.tf
  ...
```

This is deliberate, not an omission: these repos are small enough that a
module abstraction would add indirection without buying reuse. Don't
introduce `modules/` speculatively — do it when a second, genuinely
independent root config needs to share resource definitions with the
first, not before.

A repo with more than one independently-deployed concern gets more than
one root config instead of one shared module, e.g. `terraform/aws/` for
application infrastructure and `terraform/github/` for repository
administration, each with its own state and provider block.

## Backend

Default: **S3 with native locking**, no DynamoDB lock table (requires
Terraform ≥1.10):

```hcl
terraform {
  backend "s3" {
    bucket               = "shared-<org>-<repo>-s3-bucket-terraform-tfstate"
    workspace_key_prefix = "env"
    key                  = "<component>/terraform.tfstate"
    region               = "ap-northeast-1"
    encrypt              = true
    use_lockfile         = true
  }
}
```

The component in `key` namespaces every root config (`"aws"`, `"github"`)
inside one shared tfstate bucket, including Terraform's special `default`
workspace. `workspace_key_prefix` then separates non-default environments.

Alternative for a small, single-root-config repo (e.g. a repo-admin-only
config with no application infrastructure): **HCP Terraform Cloud** as the
remote backend instead of S3 —

```hcl
terraform {
  cloud {
    organization = "<org>"
    hostname     = "app.terraform.io"
    workspaces {
      project = "<repo>"
      tags = ["<repo>"]
    }
  }
}
```

Pick S3 when the repo already owns AWS infrastructure (one less external
dependency); pick Terraform Cloud when it doesn't (no need to provision a
state bucket just to hold one small config).

## Environments: Terraform workspaces, not directories

`dev`/`stg`/`prod` are Terraform workspaces (`terraform workspace select
<name>`), not separate directories or separate backends. Guard against
applying to a workspace that isn't one of the expected names with a
`null_resource` postcondition — this is the standard safety pattern, use it
verbatim:

```hcl
locals {
  stage_name_list = ["dev", "stg", "prod"]
}

resource "null_resource" "validate_workspace" {
  lifecycle {
    postcondition {
      condition     = contains(local.stage_name_list, terraform.workspace)
      error_message = "Invalid workspace. Available workspaces: ${join(", ", local.stage_name_list)}"
    }
  }
}
```

Resources shared across environments (the tfstate bucket itself, a
cross-environment DNS zone) use a literal `shared-` prefix instead of
interpolating `terraform.workspace`.

## Naming convention

```text
${terraform.workspace}-<org>-<repo>-<resource-type>-<name>
```

e.g. `dev-46ki75-web-s3-bucket-web`, `${terraform.workspace}-46ki75-internal-lambda-function-http-api`,
`${terraform.workspace}-46ki75-internal-iam-role-lambda-http-api`. Keep the
resource-type token in the name even though Terraform already knows the
resource type — it's what makes the name greppable and disambiguates
same-purpose resources of different types (an IAM role and an IAM policy
for the same feature both get a `-iam-role-` / `-iam-policy-` suffix rather
than sharing a bare name).

## Provider versions

Pessimistic constraint on the provider, exact resolution committed via the
lock file — the same "declare a floor, lock the resolution" split used for
Rust (`rust-version` + `rust-toolchain.toml`) and Python
(`requires-python` + `.python-version`):

```hcl
required_providers {
  aws = {
    source  = "hashicorp/aws"
    version = "~> 6.0"
  }
}
```

Commit `.terraform.lock.hcl`. Never commit `.terraform/` or `*.tfstate*`.

## GitHub repository administration as Terraform

Repo settings, labels, and branch/tag protection are managed as Terraform,
not clicked through the GitHub UI, using the
[`integrations/github`](https://registry.terraform.io/providers/integrations/github)
provider:

- `github_repository` for repo-level settings (description, GitHub Pages
  config, `vulnerability_alerts`).
- `github_issue_label` resources for the full label taxonomy, grouped by
  comment headers (`// Common`, `// PRs`, `// Languages`) and kept in sync
  with `dependabot.yml`'s ecosystem labels.
- `github_repository_ruleset` for branch protection (required status
  checks against the default branch) **and** for release-tag protection —
  a second ruleset locking `refs/tags/v*` and `refs/tags/*-v*` against
  creation, update, or deletion by anyone except an admin bypass actor:

  ```hcl
  resource "github_repository_ruleset" "protect_release_tags" {
    name        = "protect-release-tags"
    repository  = "<repo>"
    target      = "tag"
    enforcement = "active"

    conditions {
      ref_name {
        include = ["refs/tags/v*", "refs/tags/*-v*"]
        exclude = []
      }
    }

    rules {
      creation         = true
      update           = true
      deletion         = true
      non_fast_forward = true
    }

    bypass_actors {
      actor_type  = "OrganizationAdmin"
      bypass_mode = "always"
    }
  }
  ```

  This is the actual release-tag protection mechanism for a published
  crate/package — not a CI workflow. Prefer it over trying to enforce tag
  immutability from within a GitHub Actions job.

## Document what's deliberately not managed by Terraform

Keep a plain-English `terraform/README.md` listing resources that exist but
are intentionally out of Terraform's control — a manually created tfstate
bucket (bootstrap problem: it has to exist before Terraform can use it as a
backend), specific SSM parameter paths populated out-of-band, DNS zones
owned by a registrar UI, third-party dashboard config. Silence here reads
as "everything is managed here," which is misleading the moment one
resource is intentionally excluded.

## CI

No audited org repo currently runs `terraform fmt`/`validate`/`plan` in
CI — the only automation observed is Dependabot version-bump PRs for the
`terraform` ecosystem and, in one repo, a local `lefthook` pre-commit job
running `terraform fmt` / `terraform fmt -check` on staged files. That's a
real gap, not a pattern to replicate. For a new repo, add a workflow that
at least runs `fmt -check` and `validate` on every PR touching
`terraform/`:

```yaml
name: Terraform CI

on:
  pull_request:
    paths:
      - "terraform/**"

jobs:
  validate:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        root: [terraform]
    defaults:
      run:
        working-directory: ${{ matrix.root }}
    steps:
      - uses: actions/checkout@v4
      - uses: hashicorp/setup-terraform@v3
      - run: terraform fmt -check
      - run: terraform init -backend=false
      - run: terraform validate
```

For a multi-root repo, replace the single matrix entry with each root, for
example `[terraform/aws, terraform/github]`. Each root is initialized and
validated independently without duplicating the job.

Gate an actual `terraform plan`/`apply` behind a GitHub Actions environment
with required reviewers, the same live-tier gating pattern used for Rust
and Python — applying infrastructure changes is exactly the kind of
side-effecting, credential-requiring operation that pattern exists for.
