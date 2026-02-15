#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
remote_dir="${repo_root}/migrations"
api_dir="${repo_root}/crates/api/migrations"

if [[ ! -d "${remote_dir}" ]]; then
  echo "missing directory: ${remote_dir}" >&2
  exit 1
fi

if [[ ! -d "${api_dir}" ]]; then
  echo "missing directory: ${api_dir}" >&2
  exit 1
fi

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

missing=0

for f in "${remote_files[@]}"; do
  if [[ ! -f "${api_dir}/${f}" ]]; then
    echo "missing in crates/api/migrations: ${f}" >&2
    missing=1
  fi
done

for f in "${api_files[@]}"; do
  if [[ ! -f "${remote_dir}/${f}" ]]; then
    echo "extra numeric migration in crates/api/migrations: ${f}" >&2
    missing=1
  fi
done

if [[ "${missing}" -ne 0 ]]; then
  exit 1
fi

for f in "${remote_files[@]}"; do
  if ! cmp -s "${remote_dir}/${f}" "${api_dir}/${f}"; then
    echo "migration content differs: ${f}" >&2
    diff -u "${remote_dir}/${f}" "${api_dir}/${f}" || true
    exit 1
  fi
done

echo "migration parity ok (${#remote_files[@]} files)"
