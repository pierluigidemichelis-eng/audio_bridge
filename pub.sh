#!/bin/bash
PACKAGE_NAME="com.audiotx.audio_bridge" 

echo "=== 1. Disinstallazione e Pulizia della Cache ==="
adb uninstall $PACKAGE_NAME 2>/dev/null
adb shell pm clear $PACKAGE_NAME 2>/dev/null

echo "=== 2. Installazione nuovo APK ==="
adb install -r /home/pierluigi/audiotx/audio_bridge/audio_bridge_final.apk

echo "=== 3. Svuotamento log vecchio ==="
adb logcat -c

echo "=== 4. Avvio dell'Activity Principale ==="
adb shell am start -n $PACKAGE_NAME/android.app.NativeActivity

echo "=== 5. Calcolo PID del Processo ==="
sleep 1.5
PID=$(adb shell pidof -s $PACKAGE_NAME)

if [ -z "$PID" ]; then
    echo "⚠️ L'applicazione è crashata subito all'avvio."
    adb logcat *:S AndroidRuntime:E RustPanic:V
    exit 1
fi

echo "🟢 Applicazione intercettata! PID: $PID"
echo "=== 6. Log filtrati esclusivi ==="
adb logcat --pid=$PID *:V

