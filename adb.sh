adb install -r /home/pierluigi/audiotx/audio_bridge/audio_bridge_final.apk
adb shell pm grant com.audiotx.audio_bridge android.permission.RECORD_AUDIO
adb shell appops set com.audiotx.audio_bridge RECORD_AUDIO allow

