#!/usr/bin/env bash
set -euo pipefail

mode=${1:---dry-run}
if [[ "$mode" != "--dry-run" && "$mode" != "--execute" ]]; then
  printf '%s\n' "usage: $0 [--dry-run|--execute]" >&2
  exit 2
fi

if [[ -n "$(git status --porcelain)" ]]; then
  printf '%s\n' "release requires a clean worktree" >&2
  exit 1
fi

version=$(cargo pkgid -p arborui | tr '#' ' ' | awk '{print $NF}')
if [[ -z "$version" ]]; then
  printf '%s\n' "could not determine the workspace version" >&2
  exit 1
fi

if [[ "$mode" == "--execute" \
  && ("${GITHUB_REF_TYPE:-}" != "tag" || "${GITHUB_REF_NAME:-}" != "v${version}") ]]; then
  printf '%s\n' "release requires tag ref v${version}" >&2
  exit 1
fi

if [[ "$mode" == "--execute" && "${ARBORUI_CRATES_IO_NAME_CONFIRMED:-}" != "1" ]]; then
  printf '%s\n' \
    "set ARBORUI_CRATES_IO_NAME_CONFIRMED=1 only after confirming the arborui package family" >&2
  exit 1
fi

packages=(
  arborui-core
  arborui-text
  arborui-layout
  arborui-render
  arborui-terminal
  arborui-ui
  arborui-backend-crossterm
  arborui-runtime
  arborui-widgets
  arborui-test
  arborui
)

command=(cargo +1.90.0 publish --locked --registry crates-io)
for package in "${packages[@]}"; do
  command+=(-p "$package")
done
if [[ "$mode" == "--dry-run" ]]; then
  command+=(--dry-run)
fi

"${command[@]}"
