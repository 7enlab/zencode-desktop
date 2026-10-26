#!/bin/bash
# tools/verify-release.sh - verify one release artifact against a minisign public key.
#
# Usage: tools/verify-release.sh <public-key-file> <artifact-file>
# Example: tools/verify-release.sh zencode-release.pub ZenCode-0.4.0.AppImage
#
# Expects <artifact-file>.minisig next to the artifact (as produced by
# tools/sign-release.sh). Exit 0 = valid, non-zero = invalid or error.
set -euo pipefail

PUBKEY="${1:?usage: $0 <public-key-file> <artifact-file>}"
FILE="${2:?usage: $0 <public-key-file> <artifact-file>}"

if [ ! -f "$PUBKEY" ]; then
  echo "ERROR: public key not found: $PUBKEY" >&2
  exit 1
fi
if [ ! -f "$FILE" ]; then
  echo "ERROR: artifact not found: $FILE" >&2
  exit 1
fi
if ! command -v minisign >/dev/null 2>&1; then
  echo "ERROR: minisign not installed (apt install minisign)" >&2
  exit 1
fi

SIG="$FILE.minisig"
if [ ! -f "$SIG" ]; then
  echo "ERROR: signature file missing: $SIG" >&2
  echo "A genuine ZenCode release always ships a .minisig next to each artifact." >&2
  exit 1
fi

# minisign -V verifies both the signature and the trusted comment.
if minisign -V -p "$PUBKEY" -m "$FILE" -x "$SIG"; then
  echo "OK: signature valid - $(basename "$FILE") is an authentic ZenCode release artifact."
else
  echo "FAILED: signature INVALID for $(basename "$FILE"). Do NOT install it." >&2
  exit 1
fi
