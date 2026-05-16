#!/bin/bash

# 0. Salva la cartella di partenza del progetto
DIR=$(pwd)

# 1. Forza la variabile d'ambiente per l'NDK corretto
export ANDROID_NDK_HOME=/home/pierluigi/Android/Sdk/ndk/25.2.9519653

# 2. Compila la libreria nativa Rust solo come libreria condivisa
echo "=== Compilazione Rust in corso ==="
cargo ndk -t aarch64-linux-android build --release --lib

# Interrompe lo script se la compilazione Rust fallisce (evita APK corrotti)
if [ $? -ne 0 ]; then
    echo "Errore durante la compilazione Rust. Interruzione."
    exit 1
fi

# 3. Aggiorna il file .so appena compilato dentro la cartella build_apk
cp "$DIR/target/aarch64-linux-android/release/libaudio_bridge.so" "$DIR/build_apk/lib/arm64-v8a/"

# 4. Spostati nei build-tools di Android per generare la struttura dell'APK
cd /home/pierluigi/Android/Sdk/build-tools/30.0.3/

echo "=== Cancellazione vecchio APK base per svuotare la cache di aapt ==="
rm -f "$DIR/audio_bridge_base.apk"
rm -f "$DIR/audio_bridge_aligned.apk"
rm -f "$DIR/audio_bridge_final.apk"

echo "=== Generazione APK base pulito con AAPT ==="
./aapt package -f \
    -M "$DIR/build_apk/AndroidManifest.xml" \
    -S "$DIR/build_apk/res" \
    -I /home/pierluigi/Android/Sdk/platforms/android-30/android.jar \
    -F "$DIR/audio_bridge_base.apk"


# 5. Inserisci le librerie native .so senza applicare compressione (compressione zero)
cd "$DIR/build_apk/"
echo "=== Confezionamento librerie native nello ZIP ==="
zip -0 -r "$DIR/audio_bridge_base.apk" lib/

# 6. Torna nei build-tools per ottimizzare l'allineamento hardware a 4 byte
cd /home/pierluigi/Android/Sdk/build-tools/30.0.3/

echo "=== Ottimizzazione dei byte con ZIPALIGN ==="
./zipalign -f -v 4 "$DIR/audio_bridge_base.apk" "$DIR/audio_bridge_aligned.apk"

# 7. Firma finale dell'APK con la chiave di debug locale
echo "=== Firma del pacchetto con APKSIGNER ==="
./apksigner sign --min-sdk-version 29 \
    --ks ~/debug.keystore \
    --ks-pass pass:android \
    --out "$DIR/audio_bridge_final.apk" \
    "$DIR/audio_bridge_aligned.apk"

# 8. Ritorna alla cartella originale del progetto
cd "$DIR"
echo "=== Procedura completata! APK pronto in: audio_bridge_final.apk ==="

