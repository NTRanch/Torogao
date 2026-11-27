#!/bin/sh
# Fetch the pinned estimator models from Google's official MediaPipe
# model storage and verify their SHA-256 pins. Models are NOT
# committed to the repo (licence: Apache-2.0 per Google's official
# BlazePose GHUM / face / hand model cards, verified 2026-11-27).
set -eu
cd "$(dirname "$0")/models"
base=https://storage.googleapis.com/mediapipe-models
curl -sSLO $base/pose_landmarker/pose_landmarker_heavy/float16/1/pose_landmarker_heavy.task
curl -sSLO $base/face_landmarker/face_landmarker/float16/1/face_landmarker.task
curl -sSLO $base/hand_landmarker/hand_landmarker/float16/1/hand_landmarker.task
sha256sum -c <<'EOF'
64437af838a65d18e5ba7a0d39b465540069bc8aae8308de3e318aad31fcbc7b  pose_landmarker_heavy.task
64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff  face_landmarker.task
fbc2a30080c3c557093b5ddfc334698132eb341044ccee322ccf8bcf3607cde1  hand_landmarker.task
EOF
echo "models pinned and verified"
