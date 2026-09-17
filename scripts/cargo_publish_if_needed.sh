#!/usr/bin/env bash
# Publish one workspace crate, unless the workspace version is already there.
#
# This is what makes publish.yml idempotent. A release publishes eight crates in
# four waves with index waits between them, so a transient failure anywhere in
# the middle leaves some crates up and some not. Re-dispatching the workflow
# then has to skip what already landed rather than fail on it: `cargo publish`
# treats an already-published version as an error, which would otherwise make
# "finish the half-published release" impossible without hand-editing the job.
set -euo pipefail

crate="$1"
ver=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

already=$(curl -s -A "lanok-release (https://github.com/everruns/lanok)" \
  "https://crates.io/api/v1/crates/${crate}" \
  | python3 -c "import sys,json; print('${ver}' in {v['num'] for v in json.load(sys.stdin).get('versions',[])})" 2>/dev/null || echo False)

if [ "$already" = "True" ]; then
  echo "${crate} ${ver} already on crates.io, skipping"
else
  echo "Publishing ${crate} ${ver}"
  cargo publish -p "${crate}"
fi
