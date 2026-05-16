#!/bin/bash
DIR=$(pwd)

unset CC CXX AR TARGET_CC TARGET_AR CFLAGS CXXFLAGS RUSTFLAGS
unset CC_aarch64_linux_android CXX_aarch64_linux_android AR_aarch64_linux_android

export ANDROID_NDK_HOME=/home/pierluigi/Android/Sdk/ndk/25.2.9519653
TOOLCHAIN=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin

# Compilatori unificati reali
export CC_aarch64_linux_android="$TOOLCHAIN/clang"
export CXX_aarch64_linux_android="$TOOLCHAIN/clang++"
export AR_aarch64_linux_android="$TOOLCHAIN/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/clang"

# Flag di compilazione C/C++
export CFLAGS_aarch64_linux_android="--target=aarch64-linux-android29"
export CXXFLAGS_aarch64_linux_android="--target=aarch64-linux-android29"

# --- IL FIX: PERCORSO REALE 14.0.6 E ESCLUSIONE LIBRERIE VECCHIE ---
CLANG_LIB_DIR="/home/pierluigi/Android/Sdk/ndk/25.2.9519653/toolchains/llvm/prebuilt/linux-x86_64/lib64/clang/14.0.6/lib/linux"

# -nodefaultlibs impedisce a Rust di passare il vecchio parametro -lgcc che manda in crash Clang 14
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=--target=aarch64-linux-android29 -C link-arg=-nodefaultlibs -C link-arg=-L$CLANG_LIB_DIR -C link-arg=-l:libclang_rt.builtins-aarch64-android.a -C link-arg=-lc -C link-arg=-lm -C link-arg=-ldl -C link-arg=-llog -C link-arg=-landroid"
# -------------------------------------------------------------------

#cargo clean
#rm -f Cargo.lock

# Compilazione diretta tramite Cargo
cargo build --release --target aarch64-linux-android --bin audio_bridge

