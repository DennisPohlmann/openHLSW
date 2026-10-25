#!/bin/sh
# Sets the version of openHLSW in Cargo.toml and Cargo.lock.
# Called by semantic-release (.releaserc.json) when it makes a release:
#   tools/set-version.sh 0.3.0
set -eu

version="${1:?usage: tools/set-version.sh <version>}"
case "$version" in
  *[!0-9A-Za-z.+-]* | "") echo "invalid version: $version" >&2; exit 1 ;;
esac
cd "$(dirname "$0")/.."

# The first "version = ..." line in Cargo.toml is the one of [package].
sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
# The openhlsw entry in Cargo.lock: the "version" line after 'name = "openhlsw"'.
sed -i "/^name = \"openhlsw\"\$/{n;s/^version = \".*\"/version = \"$version\"/}" Cargo.lock

grep -m1 "^version = \"$version\"" Cargo.toml >/dev/null
grep -A1 '^name = "openhlsw"$' Cargo.lock | grep "^version = \"$version\"" >/dev/null
