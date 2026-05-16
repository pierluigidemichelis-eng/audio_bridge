sed -i 's/crate-type = /#crate-type = /' Cargo.toml
PKG_CONFIG_ALLOW_CROSS=1 \
PKG_CONFIG_PATH=$(pwd)/deps_arm64/usr/lib/aarch64-linux-gnu/pkgconfig \
PKG_CONFIG_SYSROOT_DIR=$(pwd)/deps_arm64 \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
RUSTFLAGS="-L $(pwd)/deps_arm64/usr/lib/aarch64-linux-gnu -C link-arg=-Wl,-rpath-link,$(pwd)/deps_arm64/usr/lib/aarch64-linux-gnu" \
cargo build --target aarch64-unknown-linux-gnu --bin audio_bridge --release
sed -i 's/#crate-type = /crate-type = /' Cargo.toml

