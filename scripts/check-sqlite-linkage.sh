#!/usr/bin/env bash
# Story 2.2 linkage check: `arena-cloud` links exactly one SQLite C library (rusqlite's bundled
# `libsqlite3-sys`). libsql is built remote-only, so it must bring no C core of its own.
#
# Fails unless:
#   - the binary defines exactly one `sqlite3_open_v2` (nm);
#   - it defines no `libsql_`-prefixed C symbols (Rust's mangled `_ZN…libsql_…` names are not C
#     symbols and are ignored);
#   - it loads no shared libsqlite3 (`otool -L` on macOS, `ldd` on Linux);
#   - `cargo tree -p arena-cloud` has no `libsql-ffi` / `libsql-sys`.
#
# Usage:
#   scripts/check-sqlite-linkage.sh              build arena-cloud (debug) and check it
#   scripts/check-sqlite-linkage.sh --binary P   check an already built binary P
#   scripts/check-sqlite-linkage.sh --symbols F  check an `nm` listing in F (symbol rules only)
#   scripts/check-sqlite-linkage.sh --self-test  prove the rules fire on crafted listings
# Linux and macOS (nm from binutils or Xcode/LLVM).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Reads an `nm` listing on stdin; prints what is wrong (nothing when it is fine).
symbol_problems() {
  awk '
    # Defined symbols only: the type column is the second-to-last field and is not U/w/v.
    NF >= 2 {
      name = $NF; type = $(NF - 1)
      if (type == "U" || type == "u" || type == "w" || type == "v") next
      sub(/^_/, "", name)   # Mach-O prefixes C symbols with an underscore
      if (name == "sqlite3_open_v2") opens++
      if (name ~ /^libsql_/) { libsql++; if (libsql <= 5) first = first "\n    " $NF }
    }
    END {
      if (opens != 1) printf "expected exactly one defined sqlite3_open_v2, found %d\n", opens
      if (libsql > 0) printf "found %d libsql_-prefixed C symbols:%s\n", libsql, first
    }
  '
}

check_symbols() {
  if ! grep -q '[^[:space:]]' "$1"; then
    echo "no symbols (stripped?): nm listed nothing" >&2
    return 1
  fi
  local problems
  problems="$(symbol_problems < "$1")"
  if [[ -n "$problems" ]]; then
    printf '%s\n' "$problems" >&2
    return 1
  fi
}

# Reads `cargo tree` output on stdin; fails when it has a libsql C core.
tree_problems() {
  local tree
  tree="$(cat)"
  if grep -Eq 'libsql-(ffi|sys)' <<< "$tree"; then
    echo "cargo tree -p arena-cloud contains a libsql C core (libsql-ffi / libsql-sys)" >&2
    return 1
  fi
}

check_tree() {
  local tree
  # Captured first: piping into `grep -q` under pipefail can SIGPIPE cargo and hide a match,
  # and a failing `cargo tree` must fail the check, not pass it.
  if ! tree="$(cargo tree --locked -p arena-cloud -e normal --manifest-path "$root/Cargo.toml")"; then
    echo "cargo tree -p arena-cloud failed" >&2
    return 1
  fi
  tree_problems <<< "$tree"
}

# Fails when the binary loads a shared libsqlite3 (a second, system SQLite).
check_dynamic() {
  local libs
  case "$(uname -s)" in
    Darwin) libs="$(otool -L "$1")" || { echo "otool -L failed on $1" >&2; return 1; } ;;
    *) libs="$(ldd "$1" 2>&1)" || {
      # ldd exits non-zero for a static binary ("not a dynamic executable"): nothing linked.
      grep -q 'not a dynamic executable' <<< "$libs" && return 0
      echo "ldd failed on $1" >&2
      return 1
    } ;;
  esac
  if grep -Eiq 'libsqlite3' <<< "$libs"; then
    echo "the binary links a shared libsqlite3:" >&2
    grep -Ei 'libsqlite3' <<< "$libs" >&2
    return 1
  fi
}

self_test() {
  local dir
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' RETURN
  # One copy (ELF and Mach-O spellings), Rust symbols mentioning libsql, undefined references.
  cat > "$dir/one.txt" <<'EOF'
0000000000a1b2c3 T sqlite3_open_v2
0000000000a1b2d0 T sqlite3_close
                 U sqlite3_open_v2
0000000000400000 T _ZN6libsql8database8Database7connect17h0123456789abcdefE
0000000100437870 T __ZN100_$LT$libsql_sqlite3_parser..parser..ast..UnaryOperator$GT$4from17hE
EOF
  printf '00000001007142e4 T _sqlite3_open_v2\n' > "$dir/one-macho.txt"
  # Two SQLite copies: a second (local) definition.
  cat > "$dir/two.txt" <<'EOF'
0000000000a1b2c3 T sqlite3_open_v2
0000000000c0ffee t sqlite3_open_v2
EOF
  # A libsql C core: its prefixed C API.
  cat > "$dir/libsql.txt" <<'EOF'
0000000000a1b2c3 T sqlite3_open_v2
0000000000b00000 T libsql_open
0000000000b00010 T _libsql_wal_methods_register
EOF
  : > "$dir/none.txt"

  local failed=0
  expect() { # expect pass|fail file
    if check_symbols "$dir/$2" 2> "$dir/err"; then got=pass; else got=fail; fi
    if [[ "$got" == "$1" ]]; then
      echo "ok: $2 -> $got"
    else
      echo "FAIL: $2 -> $got, expected $1" >&2
      cat "$dir/err" >&2
      failed=1
    fi
  }
  expect pass one.txt
  expect pass one-macho.txt
  expect fail two.txt
  expect fail libsql.txt
  expect fail none.txt

  # cargo tree rules.
  printf 'arena-cloud v0.1.0\n├── libsql v0.10.0\n│   ├── libsql-hrana v0.10.0\n└── rusqlite v0.40.2\n    └── libsqlite3-sys v0.38.2\n' > "$dir/tree-ok.txt"
  printf 'arena-cloud v0.1.0\n├── libsql v0.10.0\n│   └── libsql-ffi v0.9.0\n' > "$dir/tree-ffi.txt"
  expect_tree() { # expect_tree pass|fail file
    if tree_problems < "$dir/$2" 2> "$dir/err"; then got=pass; else got=fail; fi
    if [[ "$got" == "$1" ]]; then
      echo "ok: $2 -> $got"
    else
      echo "FAIL: $2 -> $got, expected $1" >&2
      failed=1
    fi
  }
  expect_tree pass tree-ok.txt
  expect_tree fail tree-ffi.txt
  return "$failed"
}

case "${1:-}" in
  --self-test)
    self_test
    echo "check-sqlite-linkage self-test passed"
    ;;
  --symbols)
    check_symbols "${2:?--symbols needs a file}"
    echo "symbols ok: ${2}"
    ;;
  --binary | "")
    if [[ "${1:-}" == "--binary" ]]; then
      bin="${2:?--binary needs a path}"
    else
      cargo build --locked -p arena-cloud --manifest-path "$root/Cargo.toml"
      bin="${CARGO_TARGET_DIR:-$root/target}/debug/arena-cloud"
    fi
    listing="$(mktemp)"
    trap 'rm -f "$listing"' EXIT
    nm "$bin" > "$listing"
    status=0
    check_symbols "$listing" || status=1
    check_dynamic "$bin" || status=1
    check_tree || status=1
    if [[ "$status" -ne 0 ]]; then
      echo "check-sqlite-linkage: FAILED for $bin" >&2
      exit 1
    fi
    echo "check-sqlite-linkage: one SQLite (rusqlite bundled), no libsql C core: $bin"
    ;;
  *)
    echo "usage: $0 [--binary <path> | --symbols <nm listing> | --self-test]" >&2
    exit 64
    ;;
esac
