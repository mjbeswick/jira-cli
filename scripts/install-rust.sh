#!/bin/sh
set -eu

# Install the Rust Jira CLI globally for the current Cargo user.
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if ! command -v cargo >/dev/null 2>&1; then
  >&2 printf '%s\n' 'Cargo is required. Install Rust using https://rustup.rs/ before running this script.'
  exit 1
fi

cargo install --path "$root" --locked --force
printf '%s\n' 'Installed Rust jira into Cargo bin directory. Ensure ~/.cargo/bin is on PATH.'
