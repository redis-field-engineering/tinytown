#!/usr/bin/env bash

set -euo pipefail

TOWN_DIR="${TINYTOWN_TOWN_DIR:-/workspace}"
TOWN_NAME="${TINYTOWN_TOWN_NAME:-tinytown-docker}"
AGENT_NAME="${TINYTOWN_AGENT_NAME:-$(hostname)}"
AGENT_CLI="${TINYTOWN_AGENT_CLI:-codex}"
AGENT_ROLE="${TINYTOWN_AGENT_ROLE:-worker}"
AGENT_MAX_ROUNDS="${TINYTOWN_AGENT_MAX_ROUNDS:-200}"

if [[ -z "${REDIS_URL:-}" ]]; then
  echo "REDIS_URL must be set for the agent worker container." >&2
  exit 1
fi

mkdir -p "${TOWN_DIR}"

if [[ ! -f "${TOWN_DIR}/tinytown.toml" ]]; then
  tt init --town "${TOWN_DIR}" --name "${TOWN_NAME}"
fi

exec tt --town "${TOWN_DIR}" spawn "${AGENT_NAME}" \
  --cli "${AGENT_CLI}" \
  --role "${AGENT_ROLE}" \
  --max-rounds "${AGENT_MAX_ROUNDS}" \
  --foreground
