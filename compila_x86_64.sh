sed -i 's/crate-type = /#crate-type = /' Cargo.toml
cargo build --bin audio_bridge --release
cargo build --target x86_64-pc-windows-gnu --bin keygen_gui --release
sed -i 's/#crate-type = /crate-type = /' Cargo.toml

