#!/usr/bin/env bash
# Runs locally what the CI workflow (.github/workflows/build.yml) can fail on, before a push.
#
#   tools/check-all.sh          fast checks (for the pre-push hook)
#   tools/check-all.sh --full   also release builds of the Linux packages with cross (docker)
#
# Covered: the CI "check" job (clippy -D warnings, tests) and compiling for every target
# the package jobs build (Windows x64/x86/arm64, macOS arm64/x86_64, Linux aarch64/riscv64,
# FreeBSD). Not covered (needs Windows/macOS or the real runners): linking against the
# Windows/macOS system libraries, WiX/.msi, lipo/codesign/.dmg, Flatpak, semantic-release.
#
# One-time setup: rustup target add <the targets below>
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

TARGETS=(
    x86_64-pc-windows-msvc
    i686-pc-windows-msvc
    aarch64-pc-windows-msvc
    aarch64-apple-darwin
    x86_64-apple-darwin
    aarch64-unknown-linux-gnu
    riscv64gc-unknown-linux-gnu
    x86_64-unknown-freebsd
)

full=0
[[ ${1:-} == --full ]] && full=1

failed=()
step() {
    local name=$1
    shift
    printf '\n== %s\n' "$name"
    if ! "$@"; then
        failed+=("$name")
    fi
}

missing=()
installed=$(rustup target list --installed)
for t in "${TARGETS[@]}"; do
    grep -qx "$t" <<<"$installed" || missing+=("$t")
done
if ((${#missing[@]})); then
    echo "Missing Rust targets, install once with:" >&2
    echo "  rustup target add ${missing[*]}" >&2
    exit 1
fi

# CI does not check the formatting; only report it.
cargo fmt --check >/dev/null 2>&1 || echo "note: cargo fmt would reformat files (not checked by CI)"

step "clippy (linux x86_64)" cargo clippy --all-targets --quiet -- -D warnings
step "tests" cargo test --quiet
for t in "${TARGETS[@]}"; do
    step "check $t" cargo check --quiet --bins --target "$t"
done

if ((full)); then
    for t in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu riscv64gc-unknown-linux-gnu; do
        step "cross build --release $t" cross build --release --target "$t"
    done
fi

if ((${#failed[@]})); then
    printf '\nFAILED: %s\n' "${failed[@]}" >&2
    exit 1
fi
printf '\nAll checks passed.\n'
