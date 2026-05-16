sed -i 's/crate-type = /#crate-type = /' Cargo.toml
cargo run --bin keygen_gui --release
sed -i 's/#crate-type = /crate-type = /' Cargo.toml

