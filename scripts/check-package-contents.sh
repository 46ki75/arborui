#!/usr/bin/env bash
set -euo pipefail

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

for package in "${packages[@]}"; do
  contents=$'\n'$(cargo package -p "$package" --list --locked --allow-dirty)$'\n'
  for required in Cargo.toml README.md LICENSE-MIT LICENSE-APACHE src/lib.rs; do
    if [[ "$contents" != *$'\n'"$required"$'\n'* ]]; then
      printf '%s\n' "$package package is missing $required" >&2
      exit 1
    fi
  done
done
