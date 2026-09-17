---
name: release
description: Cut a lanok release. Runs the publish-readiness gates, shows a human the highlights, and only after they approve prepares the version bump, pushes to main, and watches CI publish the eight crates and both SDKs. Use when the user asks to release, cut a version, or publish lanok.
metadata:
  internal: true
user-invocable: true
---

# Release

Goal: get one version of lanok onto crates.io (eight crates), PyPI, and npm,
with a human having read and approved what is being released before anything is
written.

[`knowledge/specs/release-process.md`](../../../knowledge/specs/release-process.md)
owns the versioning rules, the publish order and the CI contract. This skill
owns the procedure.

**The approval gate is the point.** Lanok has no pull requests, so there is no
merge for a reviewer to gate on. The gate is this skill asking, and it is
load-bearing: after the prep commit lands on main, everything else is automatic
and crates.io has no unpublish. Nothing in step 4 happens before a human says
yes to step 3.

## 1. Sync, and refuse to release a dirty tree

```bash
git fetch --unshallow origin main 2>/dev/null || git fetch origin main
git fetch --tags
git checkout main && git reset --hard origin/main   # only with a clean tree
git status --short                                  # must be empty
```

A shallow clone silently drops older commits from `git log`, which makes the
highlights wrong rather than absent. If `git describe --tags --abbrev=0` fails
and this is not the first release, the clone is still shallow.

## 2. Run every gate, before writing anything

```bash
just check          # fmt, clippy -D warnings, tests, OKF, schema drift
just conform        # the vectors against Rust, Python and TypeScript
just publish-dry-run
```

All three must be green on the commit being released. `publish-dry-run` is one
workspace invocation on purpose; the spec explains why per-crate dry runs cannot
work at a version bump.

Then confirm the release can actually land:

```bash
# crates.io must not already serve the version you are about to publish.
python3 scripts/verify_crates_publish.py --expected "$VERSION" --timeout 1 lanok || true
```

A `FAIL` line here is what you want: it means the version is not taken yet.

## 3. Draft the highlights and ask the human

Build the change set mechanically, never from memory:

```bash
LATEST=$(git describe --tags --abbrev=0 2>/dev/null || echo "")
git log ${LATEST:+$LATEST..}HEAD --pretty=format:'%s' --reverse \
  | grep -v '^chore(release): prepare v'
```

Reconcile it against `CHANGELOG.md`'s `## [Unreleased]` section. A commit that
changed behaviour and is missing from the changelog is a changelog bug: fix it
now, because the changelog section becomes the GitHub release notes verbatim.

Then put it to the human with `AskUserQuestion`, in one message carrying:

- the **version** being cut, and why that number under the spec's rules,
- the **highlights**, in the user's words rather than commit subjects: what
  changed for someone building a protocol on lanok, breaking changes called out
  first,
- **what will publish**: the eight crates, the Python SDK, the TypeScript SDK,
  all at that version,
- the **gate results** from step 2,
- anything from the prerequisites table below that is not in place.

Offer: approve, approve with edits to the highlights, or cancel. On cancel,
stop and leave the tree untouched. On edits, make them and ask again.

## 4. Prepare the release, only now

```bash
# The changelog: move [Unreleased] under a version heading with today's date.
# Keep an empty [Unreleased] above it for the next cycle.

# The version, in all four places the spec names:
#   - [workspace.package] version
#   - every [workspace.dependencies] lanok* entry
#   - sdks/python/pyproject.toml
#   - sdks/typescript/package.json
cargo update -w   # refresh the lockfile entries for the bumped crates
```

Re-run `just check` after the bump, then commit **directly to main** and push.
The subject line is the trigger, so it is exact:

```bash
git add CHANGELOG.md Cargo.toml Cargo.lock sdks/python/pyproject.toml sdks/typescript/package.json
git commit -m "chore(release): prepare vX.Y.Z"
git push origin main
```

## 5. Watch it land, and verify rather than trust

`release.yml` fires on that commit subject: it re-checks every version against
the tag, tags `vX.Y.Z`, creates the GitHub release from the changelog section,
and dispatches `publish.yml`, which publishes the crates in four waves with
index waits, plus both SDKs.

```bash
gh run list --workflow=release.yml --limit 1
gh run list --workflow=publish.yml --limit 1
```

Green workflows are not proof. Verify the registries themselves:

```bash
python3 scripts/verify_crates_publish.py --expected "$VERSION" \
  lanok-core lanok-macros lanok-transport lanok-schema lanok-clap \
  lanok-peer lanok-cli lanok
curl -s https://pypi.org/pypi/lanok/json | python3 -c "import sys,json; print(json.load(sys.stdin)['info']['version'])"
npm view @lanok/rpc version
```

Declare **shipped** only when all three registries report the version. On
failure, read the logs (`gh run view <id> --log-failed`) and re-dispatch
`publish.yml`: every publish step skips what is already up, so a re-run fills
the gaps rather than failing on them. Never leave a release half-published.

## Prerequisites, one-time

| What | Where | For |
|---|---|---|
| `CARGO_REGISTRY_TOKEN` | repo secret | the eight crates |
| `release` environment | repo settings | every publish job runs in it |
| PyPI trusted publisher | pypi.org, project `lanok` | owner `everruns`, repo `lanok`, workflow `publish.yml`, environment `release` |
| npm trusted publisher | npmjs.com, package `@lanok/rpc` | the `lanok` scope must exist; same repo/workflow/environment |

The two trusted publishers use OIDC, so there is no npm or PyPI token to store.
Both must be registered before the first release, or those jobs fail while the
crates go up, which is a half-published release.

## After the first publish

mira and yolop pin lanok as a git dependency "only while lanok is unpublished".
Once a version is live, move them to version requirements. That is a change in
those repositories, not this one.
