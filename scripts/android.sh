#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../android"
export JAVA_HOME="${JAVA_HOME:-$HOME/android-studio/jbr}"
nice -n 19 ./gradlew --no-daemon -q assembleRelease
declare -A APKS=([app]=app/build/outputs/apk/release/app-release.apk [assistant]=assistant/build/outputs/apk/release/assistant-release.apk)
for m in app assistant; do echo "Built $(realpath "${APKS[$m]}")"; done
if [ "${1:-}" = "install" ]; then
    for m in ${2:-app assistant}; do adb install -r "${APKS[$m]}"; done
fi
