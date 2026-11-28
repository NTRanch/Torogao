#!/bin/sh
# torogao-rig-engine launcher (the user service runs this).
# State and tools live beside the deployed copy unless the
# environment overrides them.
set -eu
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export TOROGAO_RIG_ENGINE_HOME="${TOROGAO_RIG_ENGINE_HOME:-$HERE}"
export TOROGAO_RIG_ENGINE_TOOLS="${TOROGAO_RIG_ENGINE_TOOLS:-$HERE/tools}"
exec python3 "$HERE/rig_engine.py"
