#!/bin/sh
# Select mold, lld, or the compiler's default linker in that order.
if command -v clang >/dev/null 2>&1; then CC=clang; else CC=cc; fi
if command -v mold >/dev/null 2>&1; then
  exec "$CC" -fuse-ld=mold "$@"
elif command -v ld.lld >/dev/null 2>&1; then
  exec "$CC" -fuse-ld=lld "$@"
else
  exec "$CC" "$@"
fi
