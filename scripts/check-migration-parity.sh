#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
canonical_dir="${repo_root}/crates/api/migrations"
mirror_dir="${repo_root}/migrations"

if [[ ! -d "${canonical_dir}" ]]; then
  echo "missing directory: ${canonical_dir}" >&2
  exit 1
fi

if [[ ! -d "${mirror_dir}" ]]; then
  echo "missing directory: ${mirror_dir}" >&2
  exit 1
fi

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

missing=0

for f in "${canonical_files[@]}"; do
  if [[ ! -f "${mirror_dir}/${f}" ]]; then
    echo "missing in migrations/: ${f}" >&2
    missing=1
  fi
done

for f in "${mirror_files[@]}"; do
  if [[ ! -f "${canonical_dir}/${f}" ]]; then
    echo "extra numeric migration in migrations/: ${f}" >&2
    missing=1
  fi
done

if [[ "${missing}" -ne 0 ]]; then
  exit 1
fi

for f in "${canonical_files[@]}"; do
  if ! cmp -s "${canonical_dir}/${f}" "${mirror_dir}/${f}"; then
    echo "migration content differs: ${f}" >&2
    diff -u "${canonical_dir}/${f}" "${mirror_dir}/${f}" || true
    exit 1
  fi
done

echo "migration parity ok (${#canonical_files[@]} files, canonical: crates/api/migrations)"
