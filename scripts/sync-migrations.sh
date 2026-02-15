#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
remote_dir="${repo_root}/migrations"
api_dir="${repo_root}/crates/api/migrations"

remote_files=()
while IFS= read -r f; do
  remote_files+=("${f}")
done < <(cd "${remote_dir}" && ls -1 [0-9][0-9][0-9][0-9]_*.sql 2>/dev/null | sort)

api_files=()
while IFS= read -r f; do
  api_files+=("${f}")
done < <(cd "${api_dir}" && ls -1 [0-9][0-9][0-9][0-9]_*.sql 2>/dev/null | sort)

if [[ "${#remote_files[@]}" -eq 0 ]]; then
  echo "no remote migrations found in ${remote_dir}" >&2
  exit 1
fi

for f in "${remote_files[@]}"; do
  cp "${remote_dir}/${f}" "${api_dir}/${f}"
  echo "synced ${f}"
done

for f in "${api_files[@]}"; do
  if [[ ! -f "${remote_dir}/${f}" ]]; then
    rm -f "${api_dir}/${f}"
    echo "removed stale ${f}"
  fi
done

echo "sync complete"
