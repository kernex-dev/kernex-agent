#!/bin/sh
# Bootstrap default skills and workflows into the data volume on first run.
# On subsequent starts the .initialized marker skips this block.
set -e

KX_DATA="/home/kx/.kx"
DEFAULTS="/usr/share/kx/defaults"
MARKER="${KX_DATA}/.initialized"

if [ ! -f "${MARKER}" ]; then
  if [ -d "${DEFAULTS}/skills" ] && [ ! -d "${KX_DATA}/skills" ]; then
    cp -r "${DEFAULTS}/skills" "${KX_DATA}/skills"
  fi
  if [ -d "${DEFAULTS}/workflows" ] && [ ! -d "${KX_DATA}/workflows" ]; then
    cp -r "${DEFAULTS}/workflows" "${KX_DATA}/workflows"
  fi
  touch "${MARKER}"
fi

exec kx "$@"
