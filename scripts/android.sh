#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../android"
export JAVA_HOME="${JAVA_HOME:-$HOME/android-studio/jbr}"
nice -n 19 ./gradlew --no-daemon -q assembleRelease
APK=app/build/outputs/apk/release/app-release.apk
echo "Built $(realpath "$APK")"
if [ "${1:-}" = "install" ]; then
    adb install -r "$APK"
fi
