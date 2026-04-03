#!/usr/bin/env bash

set -euo pipefail

TOWN_DIR="${TINYTOWN_TOWN_DIR:-/workspace}"
TOWN_NAME="${TINYTOWN_TOWN_NAME:-tinytown-docker}"
REST_BIND="${TINYTOWN_REST_BIND:-0.0.0.0}"
REST_PORT="${TINYTOWN_REST_PORT:-8080}"
MCP_BIND="${TINYTOWN_MCP_BIND:-0.0.0.0}"
MCP_PORT="${TINYTOWN_MCP_PORT:-8081}"
ENABLE_MCP_HTTP="${TINYTOWN_ENABLE_MCP_HTTP:-1}"

if [[ -z "${REDIS_URL:-}" ]]; then
  echo "REDIS_URL must be set for the townhall container." >&2
  exit 1
fi

mkdir -p "${TOWN_DIR}"

if [[ ! -f "${TOWN_DIR}/tinytown.toml" ]]; then
  tt init --town "${TOWN_DIR}" --name "${TOWN_NAME}"
fi

cleanup() {
  local exit_code=$?

  if [[ -n "${dispatcher_pid:-}" ]]; then
    kill "${dispatcher_pid}" 2>/dev/null || true
  fi

  if [[ -n "${mcp_pid:-}" ]]; then
    kill "${mcp_pid}" 2>/dev/null || true
  fi

  wait 2>/dev/null || true
  exit "${exit_code}"
}

trap cleanup EXIT INT TERM

pids=()

tt --town "${TOWN_DIR}" mission dispatch &
dispatcher_pid=$!
pids+=("${dispatcher_pid}")

if [[ "${ENABLE_MCP_HTTP}" != "0" ]]; then
  townhall --town "${TOWN_DIR}" mcp-http --bind "${MCP_BIND}" --port "${MCP_PORT}" &
  mcp_pid=$!
  pids+=("${mcp_pid}")
fi

townhall --town "${TOWN_DIR}" rest --bind "${REST_BIND}" --port "${REST_PORT}" &
rest_pid=$!
pids+=("${rest_pid}")

wait -n "${pids[@]}"
