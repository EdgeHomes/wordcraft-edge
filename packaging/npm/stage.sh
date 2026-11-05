#!/bin/sh
# Stage the wordcraft-edge-mcp npm packages under packaging/npm/dist.
#
#   packaging/npm/stage.sh [RUST_TARGET ...]   (default: darwin-arm64 and both static Linux musl targets)
#
# Each target becomes a platform package (wordcraft-edge-mcp-<os>-<cpu>) holding the release
# wordcraft-cli binary. The launcher package is staged last, with optionalDependencies on every
# platform package present in dist, all at the workspace version. Publish the platform packages
# first, then the launcher:
#   for d in dist/wordcraft-edge-mcp-*/ dist/wordcraft-edge-mcp/; do npm publish "$d" --access public; done
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
dist="$here/dist"
# NPM_VERSION overrides the workspace version, for npm-only releases of the same engine version.
version=${NPM_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -n 1)}
[ -n "$version" ] || { echo "stage.sh: no workspace version in Cargo.toml" >&2; exit 1; }

[ $# -gt 0 ] || set -- aarch64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl

for target in "$@"; do
  case "$target" in
    x86_64-apple-darwin) os=darwin cpu=x64 ;;
    aarch64-apple-darwin) os=darwin cpu=arm64 ;;
    # musl builds are static, so one binary runs on glibc distros and on Alpine (LibreChat's image).
    x86_64-unknown-linux-musl) os=linux cpu=x64 ;;
    aarch64-unknown-linux-musl) os=linux cpu=arm64 ;;
    x86_64-pc-windows-msvc) os=win32 cpu=x64 ;;
    aarch64-pc-windows-msvc) os=win32 cpu=arm64 ;;
    *) echo "stage.sh: unsupported target $target" >&2; exit 1 ;;
  esac
  exe=wordcraft-cli; [ "$os" = win32 ] && exe=wordcraft-cli.exe
  # Linux from macOS/Windows needs a cross linker: cargo-zigbuild (cargo install cargo-zigbuild; brew install zig).
  build=build
  case "$target" in *-linux-*) [ "$(uname -s)" = Linux ] || build=zigbuild ;; esac
  rustup target add "$target" >/dev/null
  (cd "$repo" && perl -e 'alarm 1800; exec @ARGV' cargo "$build" -p wordcraft-cli --release --locked --target "$target" -j 4)
  pkg="$dist/wordcraft-edge-mcp-$os-$cpu"
  rm -rf "$pkg" && mkdir -p "$pkg/bin"
  /bin/cp -f "${CARGO_TARGET_DIR:-$repo/target}/$target/release/$exe" "$pkg/bin/$exe"
  cat > "$pkg/package.json" <<EOF
{
  "name": "wordcraft-edge-mcp-$os-$cpu",
  "version": "$version",
  "description": "wordcraft-cli binary for wordcraft-edge-mcp ($os-$cpu)",
  "license": "MIT OR Apache-2.0",
  "repository": { "type": "git", "url": "git+https://github.com/mbarker-edgehomes/wordcraft-edge.git" },
  "os": ["$os"],
  "cpu": ["$cpu"],
  "files": ["bin"]
}
EOF
  echo "staged $pkg ($target)"
done

main="$dist/wordcraft-edge-mcp"
rm -rf "$main" && /bin/cp -f -R "$here/wordcraft-edge-mcp" "$main"
/bin/cp -f "$repo/LICENSE-MIT" "$repo/LICENSE-APACHE" "$main/"
(cd "$main" && npm pkg set version="$version")
for p in "$dist"/wordcraft-edge-mcp-*/; do
  name=$(basename "$p")
  (cd "$main" && npm pkg set "optionalDependencies.$name=$version")
done
echo "staged $main ($version)"
