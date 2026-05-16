#!/bin/bash
# Resettiamo le variabili sporche nella sessione attuale
unset RUSTFLAGS

export ANDROID_NDK_HOME=/home/pierluigi/Android/Sdk/ndk/20.1.5948944
TOOLCHAIN=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin

export CC_aarch64_linux_android=$TOOLCHAIN/aarch64-linux-android29-clang
export AR_aarch64_linux_android=$TOOLCHAIN/llvm-ar

# --- FORZATURA CHIRURGICA SULLA TUA VERSIONE REALE 8.0.7 ---
CLANG_VERSION="8.0.7"
CLANG_LIB_DIR="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/lib64/clang/$CLANG_VERSION/lib/linux"

# Passiamo il percorso esatto della tua macchina al linker di Rust
export RUSTFLAGS="-C link-arg=-L$CLANG_LIB_DIR -C link-arg=-l:libclang_rt.builtins-aarch64-android.a"
# -----------------------------------------------------------

#cargo clean
#rm -f Cargo.lock
cargo ndk -t arm64-v8a -p 29 -- build --release --bin audio_bridge

