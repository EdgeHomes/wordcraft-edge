# WordCraft npm distribution

Stage the launcher and three native packages with `RUSTUP_TOOLCHAIN=1.97.1 packaging/npm/stage.sh`.
The default targets are Darwin ARM64 and statically linked Linux x64/ARM64 musl. Linux cross
builds on macOS use `cargo zigbuild` and Zig; install their Rust targets first. Each build has
an 1800-second timeout. `NPM_VERSION` can select an npm-only version; all optional platform
dependencies get that exact version. GNU Linux targets are intentionally refused.

The staging directory `dist/` is ignored. Platform packages contain only the binary and package
metadata. The launcher is the only JavaScript: all document work, tools and confinement are Rust.

See the [package guide](wordcraft-edge-mcp/README.md) for LibreChat setup and publication, and
[VERIFICATION.md](VERIFICATION.md) for evidence and limits. No CI or deployment changes are needed.
