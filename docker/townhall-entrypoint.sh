#!/usr/bin/env bash

set -euo pipefail

TOWN_DIR="${TINYTOWN_TOWN_DIR:-/workspace}"
TOWN_NAME="${TINYTOWN_TOWN_NAME:-tinytown-docker}"
REST_BIND="${TINYTOWN_REST_BIND:-0.0.0.0}"
REST_PORT="${TINYTOWN_REST_PORT:-8080}"
MCP_BIND="${TINYTOWN_MCP_BIND:-0.0.0.0}"
MCP_PORT="${TINYTOWN_MCP_PORT:-8081}"
ENABLE_MCP_HTTP="${TINYTOWN_ENABLE_MCP_HTTP:-1}"
TOWNHALL_API_KEY_PATH="${TINYTOWN_TOWNHALL_API_KEY_PATH:-${TOWN_DIR}/.townhall-api-key}"

if [[ -z "${REDIS_URL:-}" ]]; then
  echo "REDIS_URL must be set for the townhall container." >&2
  exit 1
fi

mkdir -p "${TOWN_DIR}"

if [[ ! -f "${TOWN_DIR}/tinytown.toml" ]]; then
  tt init --town "${TOWN_DIR}" --name "${TOWN_NAME}"
fi

is_loopback_bind() {
  case "$1" in
    127.*|::1)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

current_auth_mode() {
  awk '
    /^\[townhall\.auth\]/ { in_auth=1; next }
    /^\[/ && in_auth { exit }
    in_auth && /^mode = / {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "${TOWN_DIR}/tinytown.toml"
}

configure_remote_rest_auth() {
  local auth_mode auth_output raw_key api_key_hash tmp_config

  if is_loopback_bind "${REST_BIND}"; then
    return
  fi

  auth_mode="$(current_auth_mode)"
  if [[ -n "${auth_mode}" && "${auth_mode}" != "none" ]]; then
    return
  fi

  auth_output="$(tt auth gen-key --town "${TOWN_DIR}" 2>&1)"
  raw_key="$(printf '%s\n' "${auth_output}" | awk '/API Key \(store securely, shown only once\):/{getline; print; exit}')"
  api_key_hash="$(printf '%s\n' "${auth_output}" | awk '/API Key Hash \(add to tinytown.toml\):/{getline; print; exit}')"

  if [[ -z "${raw_key}" || -z "${api_key_hash}" ]]; then
    printf '%s\n' "${auth_output}" >&2
    echo "Failed to generate Townhall API key for non-loopback REST bind." >&2
    exit 1
  fi

  tmp_config="$(mktemp)"
  awk -v hash="${api_key_hash}" '
    BEGIN { in_auth=0; inserted=0; saw_auth=0 }
    /^\[townhall\.auth\]/ {
      in_auth=1
      saw_auth=1
      print
      next
    }
    /^\[/ && in_auth {
      if (!inserted) {
        print "mode = \"api_key\""
        print "api_key_hash = \"" hash "\""
        inserted=1
      }
      in_auth=0
    }
    in_auth && /^mode = / {
      print "mode = \"api_key\""
      print "api_key_hash = \"" hash "\""
      inserted=1
      next
    }
    in_auth && /^api_key_hash = / { next }
    { print }
    END {
      if (in_auth && !inserted) {
        print "mode = \"api_key\""
        print "api_key_hash = \"" hash "\""
      } else if (!saw_auth) {
        print ""
        print "[townhall.auth]"
        print "mode = \"api_key\""
        print "api_key_hash = \"" hash "\""
      }
    }
  ' "${TOWN_DIR}/tinytown.toml" > "${tmp_config}"
  mv "${tmp_config}" "${TOWN_DIR}/tinytown.toml"

  printf '%s\n' "${raw_key}" > "${TOWNHALL_API_KEY_PATH}"
  chmod 600 "${TOWNHALL_API_KEY_PATH}"

  echo "Configured API key authentication for Townhall REST on ${REST_BIND}:${REST_PORT}." >&2
  echo "Stored the generated API key at ${TOWNHALL_API_KEY_PATH}." >&2
}

configure_remote_rest_auth

cleanup() {
  local exit_code=$?

  if [[ ${#pids[@]} -gt 0 ]]; then
    kill "${pids[@]}" 2>/dev/null || true
  fi

  wait 2>/dev/null || true
  exit "${exit_code}"
}

pids=()

trap cleanup EXIT INT TERM

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
