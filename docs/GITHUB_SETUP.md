# GitHub Configuration Checklist

A one-time setup checklist for maintainers. Work top-down; each section is independent
once its predecessors are done. Most items are clicks in the GitHub web UI — paths are
given as `Settings → Section → Item`.

Conventions:

- `gh` commands assume the [GitHub CLI](https://cli.github.com/) is installed and
  authenticated as a repo admin.
- Replace `OWNER/REPO` with the real slug.

---

## 1. Repository settings

- [ ] **General → Features**: disable Wikis (docs live in-repo); keep Issues, Discussions
      (optional), Projects.
- [ ] **General → Pull Requests**: allow **Squash merging** and **Rebase merging** only.
      Disable "Allow merge commits". Enable "Always suggest updating pull request
      branches" and "Automatically delete head branches".
- [ ] **General → Pull Requests**: enable "Allow auto-merge".
- [ ] **Collaborators and teams**: add maintainers with `Maintain` or `Admin`; external
      contributors stay as `Read` (they work from forks).
- [ ] **Moderation → Code review limits**: leave open (we want external PRs).
- [ ] **Actions → General → Fork pull request workflows from outside collaborators**:
      set to **Require approval for first-time contributors**.
- [ ] **Actions → General → Workflow permissions**: set to **Read repository contents
      and packages permissions** (default to read-only; workflows opt in to write).
- [ ] **Actions → General**: disable "Allow GitHub Actions to create and approve pull
      requests" unless you specifically need it.

## 2. Branch protection on `main`

`Settings → Branches → Add branch ruleset` (or classic branch protection). Apply to
`main`:

- [ ] **Restrict deletions**.
- [ ] **Block force pushes**.
- [ ] **Require linear history**.
- [ ] **Require signed commits** (optional but recommended; pair with DCO).
- [ ] **Require a pull request before merging**:
  - [ ] Required approvals: **1** (raise to **2** for security-sensitive paths via
        CODEOWNERS).
  - [ ] **Dismiss stale pull request approvals when new commits are pushed**.
  - [ ] **Require review from Code Owners**.
  - [ ] **Require approval of the most recent reviewable push**.
- [ ] **Require status checks to pass before merging** — add each required check:
  - [ ] `ci-rust`
  - [ ] `ci-java`
  - [ ] `ci-conformance`
  - [ ] `ci-docs`
  - [ ] `DCO` (after the DCO app/action is installed — see §5)
  - [ ] `CodeQL` (after CodeQL is enabled — see §6)
  - [ ] **Require branches to be up to date before merging**.
- [ ] **Require conversation resolution before merging**.
- [ ] **Do not allow bypassing the above settings** (admins included; turn off only when
      you genuinely need a hot-fix path).

## 3. CODEOWNERS

- [ ] Create `.github/CODEOWNERS`. Minimum coverage:
  - `/crates/creda-events/` → M1 owner(s)
  - `/crates/creda-store/`  → M2 owner(s)
  - `/crates/creda-graph/`  → M3 owner(s)
  - `/crates/creda-net/`    → M4 owner(s)
  - `/crates/creda-core/`   → M5 owner(s)
  - `/bridge/`              → bridge owner(s)
  - `/conformance/`         → conformance owner(s)
  - `/deploy/`              → infra owner(s)
  - `/docs/creda-technical-spec.md` → spec owner(s) (require 2 approvals)
  - `/SECURITY.md`, `**/crypto/**`, `**/udap/**`, `**/spiffe/**` → security owner(s)
        (require 2 approvals)
  - `/.github/`             → maintainers
- [ ] Confirm "Require review from Code Owners" is checked in branch protection (§2).

## 4. Issue and PR templates

- [ ] `.github/pull_request_template.md` — mirrors the CONTRIBUTING.md checklist.
- [ ] `.github/ISSUE_TEMPLATE/bug_report.yml`
- [ ] `.github/ISSUE_TEMPLATE/spec_question.yml`
- [ ] `.github/ISSUE_TEMPLATE/open_question.yml` — for tracking
      `TODO(open-question-13.x)`.
- [ ] `.github/ISSUE_TEMPLATE/config.yml` — disable blank issues; link `SECURITY.md`
      for vulnerability reports.

## 5. DCO enforcement

- [ ] Install the [DCO GitHub App](https://github.com/apps/dco) **or** add a workflow
      using `tim-actions/dco`. Either way the check name should be `DCO`.
- [ ] Add `DCO` to required status checks (§2).

## 6. Security features

- [ ] **Settings → Code security and analysis**:
  - [ ] Enable **Private vulnerability reporting**.
  - [ ] Enable **Dependency graph**.
  - [ ] Enable **Dependabot alerts**.
  - [ ] Enable **Dependabot security updates**.
  - [ ] Enable **Dependabot version updates** — commit `.github/dependabot.yml` covering
        `cargo`, `gradle` / `maven`, `github-actions`, and `docker`.
  - [ ] Enable **Secret scanning** and **Push protection**.
  - [ ] Enable **CodeQL** for Rust and Java (default setup is fine to start).
- [ ] Add `gitleaks` as a pre-commit hook (documented in `CONTRIBUTING.md`) and as a CI
      job for defense in depth.
- [ ] Pin third-party Actions to a **commit SHA**, not a tag. Audit existing workflows.

## 7. Secrets and environments

- [ ] **Settings → Secrets and variables → Actions**: confirm no secrets are exposed to
      `pull_request` jobs from forks. Use **Environments** (e.g., `release`, `deploy`)
      with required reviewers for anything that needs secrets.
- [ ] Document in `CONTRIBUTING.md` that fork PRs do not have access to secrets and that
      jobs requiring secrets only run after a maintainer pushes to a branch in the main
      repo.

## 8. Releases and tags

- [ ] Protect tags matching `m*` and `v*` (`Settings → Tags → New rule`).
- [ ] Decide on tag signing: require for release tags.
- [ ] Draft a release-notes template; auto-generate from PR titles using
      `.github/release.yml`.

## 9. Labels

- [ ] Create a minimal label set: `bug`, `enhancement`, `docs`, `security`,
      `good-first-issue`, `help-wanted`, `needs-triage`, `blocked`,
      `open-question-13.x`, and one per milestone (`M0`…`M9`).

## 10. Optional but useful

- [ ] **Stale-bot** or scheduled workflow to nudge issues/PRs with no activity in 30
      days.
- [ ] **Mergify** or **Kodiak** for queue-based merging once PR volume grows.
- [ ] **Renovate** as an alternative to Dependabot if you want richer scheduling.

---

## Quick-start commands

```sh
# Enable security features (some require admin).
gh api -X PATCH repos/OWNER/REPO \
  -f has_wiki=false \
  -f allow_merge_commit=false \
  -f allow_squash_merge=true \
  -f allow_rebase_merge=true \
  -f delete_branch_on_merge=true \
  -f allow_auto_merge=true

gh api -X PUT repos/OWNER/REPO/vulnerability-alerts
gh api -X PUT repos/OWNER/REPO/automated-security-fixes

# Verify branch protection after configuring in the UI.
gh api repos/OWNER/REPO/branches/main/protection
```

## Verification

When done, confirm each of these from a fresh fork:

1. A PR from a fork triggers CI but receives no secrets.
2. A first-time contributor's workflow is held for maintainer approval.
3. A PR missing DCO signoff fails the `DCO` check and cannot be merged.
4. A PR touching `crates/creda-core/` requires the M5 code owner.
5. Force-push to `main` is rejected.
6. Merge commit on `main` is rejected (squash/rebase only).
