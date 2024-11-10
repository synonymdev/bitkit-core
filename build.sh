#!/bin/bash

# Save as build.sh
case "$1" in
  "ios")
    ./build_ios.sh
    ;;
  "android")
    ./build_android.sh
    ;;
  "python")
    ./build_python.sh
    ;;
  "all")
    ./build_ios.sh && ./build_android.sh && ./build_python.sh
    ;;
  *)
    echo "Usage: $0 {ios|android|python|all}"
    exit 1
    ;;
esac