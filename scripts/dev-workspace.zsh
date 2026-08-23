#!/bin/zsh
set -eu

# Portable runner contract:
#   RAN_WORKSPACE_PATH  worktree to run (defaults to this script's repository)
#   RAN_ROOT_PATH       primary checkout holding ixi-config.yaml (auto-detected)
#   RAN_KUBECONFIG      kubeconfig to use (defaults to RAN_ROOT_PATH/ixi-config.yaml)
#   RAN_DEV_PORT        backend port; Vite uses the following port
script_dir="${0:A:h}"
workspace_path="${RAN_WORKSPACE_PATH:-${script_dir:h}}"

if [[ -n "${RAN_ROOT_PATH:-}" ]]; then
  root_path="$RAN_ROOT_PATH"
else
  git_common_dir="$(git -C "$workspace_path" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
  if [[ -n "$git_common_dir" && "${git_common_dir:t}" == ".git" ]]; then
    root_path="${git_common_dir:h}"
  else
    root_path="$workspace_path"
  fi
fi

backend_port="${RAN_DEV_PORT:-8080}"

if [[ "$backend_port" != <-> ]] || (( backend_port < 1 || backend_port >= 65535 )); then
  print -u2 "RAN_DEV_PORT must be an integer between 1 and 65534"
  exit 1
fi

vite_port=$((backend_port + 1))
root_kubeconfig="${RAN_KUBECONFIG:-$root_path/ixi-config.yaml}"
workspace_kubeconfig="$workspace_path/ixi-config.yaml"

if [[ ! -r "$root_kubeconfig" ]]; then
  print -u2 "Missing $root_kubeconfig"
  print -u2 "Start $root_path/ixiplay.sh first so the tunnel and kubeconfig are available."
  exit 1
fi

if [[ "$root_kubeconfig" != "$workspace_kubeconfig" ]]; then
  ln -sfn "$root_kubeconfig" "$workspace_kubeconfig"
fi

if ! KUBECONFIG="$workspace_kubeconfig" kubectl get --raw=/version --request-timeout=5s >/dev/null 2>&1; then
  print -u2 "The target cluster is not reachable through $workspace_kubeconfig"
  print -u2 "Keep $root_path/ixiplay.sh running, then try again."
  exit 1
fi

export RAN_VITE_HOST="127.0.0.1"
export RAN_VITE_PORT="$vite_port"
export RAN_VITE_ORIGIN="http://127.0.0.1:$vite_port"

typeset -a child_pids

terminate_tree() {
  local pid="$1"
  local child
  for child in $(pgrep -P "$pid" 2>/dev/null || true); do
    terminate_tree "$child"
  done
  kill -TERM "$pid" 2>/dev/null || true
}

cleanup() {
  trap - EXIT HUP INT TERM
  local pid
  for pid in $child_pids; do
    terminate_tree "$pid"
  done
  for pid in $child_pids; do
    wait "$pid" 2>/dev/null || true
  done
}

trap 'exit_code=$?; cleanup; exit $exit_code' EXIT
trap 'cleanup; exit 130' HUP INT TERM

(
  cd "$workspace_path/frontend"
  pnpm run dev
) &
child_pids+=($!)

(
  cd "$workspace_path"
  RAN_LOG="${RAN_LOG:-debug}" cargo run -p cli -- \
    emulate \
    --port "$backend_port" \
    --kubeconfig "$workspace_kubeconfig"
) &
child_pids+=($!)

print "Frontend: http://127.0.0.1:$vite_port"
print "Ran:      http://127.0.0.1:$backend_port"

while true; do
  for pid in $child_pids; do
    if ! kill -0 "$pid" 2>/dev/null; then
      set +e
      wait "$pid"
      child_status=$?
      set -e
      exit "$child_status"
    fi
  done
  sleep 1
done
