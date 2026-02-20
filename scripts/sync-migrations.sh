#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
canonical_dir="${repo_root}/crates/api/migrations"
mirror_dir="${repo_root}/migrations"

canonical_files=()
while IFS= read -r f; do
  canonical_files+=("${f}")
done < <(cd "${canonical_dir}" && ls -1 [0-9][0-9][0-9][0-9]_*.sql 2>/dev/null | sort)

mirror_files=()
while IFS= read -r f; do
  mirror_files+=("${f}")
done < <(cd "${mirror_dir}" && ls -1 [0-9][0-9][0-9][0-9]_*.sql 2>/dev/null | sort)

if [[ "${#canonical_files[@]}" -eq 0 ]]; then
  echo "no canonical migrations found in ${canonical_dir}" >&2
  exit 1
fi

for f in "${canonical_files[@]}"; do
  cp "${canonical_dir}/${f}" "${mirror_dir}/${f}"
  echo "synced ${f}"
done

for f in "${mirror_files[@]}"; do
  if [[ ! -f "${canonical_dir}/${f}" ]]; then
    rm -f "${mirror_dir}/${f}"
    echo "removed stale ${f}"
  fi
done

echo "sync complete"
