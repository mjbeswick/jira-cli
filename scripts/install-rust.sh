#!/bin/sh
set -eu

# Run from any directory; build only this repository's Rust package.
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if ! command -v cargo >/dev/null 2>&1; then
  >&2 printf '%s\n' 'Cargo is required. Install Rust using https://rustup.rs/ before running this script.'
  exit 1
fi

# Keep the candidate out of PATH until the Jira specialist is updated and live
# Rust verification is permitted. An explicit prefix can still be chosen.
prefix=${JIRA_RUST_INSTALL_ROOT:-"${HOME}/.local/share/jira-cli-rust"}
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
cargo build --release --manifest-path "$root/Cargo.toml" --locked
install -d "$prefix/bin"
install -m 755 "$target_dir/release/jira" "$prefix/bin/jira"
printf 'Installed Rust candidate at %s/bin/jira\n' "$prefix"
printf '%s\n' 'Invoke it by full path for help. The existing npm `jira` on PATH remains unchanged.'
