


set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$PATH"

echo "=== 1/4 fmt ==="
cargo fmt --all -- --check && echo "fmt clean"

echo "=== 2/4 clippy (native, -D warnings) ==="
for crate in rebuild core engine; do
  (cd "$crate" && cargo clippy --locked --all-targets -- -D warnings)
done
echo "clippy clean"

echo "=== 3/4 cross-check: engine -> x86_64-pc-windows-gnu ==="
if ! rustup target list --installed | grep -q x86_64-pc-windows-gnu; then
  rustup target add x86_64-pc-windows-gnu
fi
if ! command -v x86_64-w64-mingw32-gcc >/dev/null; then
  echo "installing mingw-w64 (needs sudo)..."
  sudo apt-get install -y mingw-w64
fi
(cd engine && cargo check --locked --target x86_64-pc-windows-gnu)
echo "windows-gnu check clean"

echo "=== 4/4 macOS ==="
echo "SKIP: x86_64-apple-darwin / aarch64-apple-darwin need the Apple SDK;"
echo "macOS CI (native) is the gate. Do not merge macOS-specific code without it."

echo "ALL LOCAL CHECKS PASSED"
