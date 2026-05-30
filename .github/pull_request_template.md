<!--
Thanks for contributing to Creda. Please fill out the sections below.
See CONTRIBUTING.md for the full guide.
-->

## Summary

<!-- One or two sentences. What does this PR change, and why? -->

## Spec reference

<!-- Cite the governing spec section(s), e.g. "Implements §5.2.4 effective-identity projection". -->

- Spec section(s):
- Milestone: M_
- Related issue: #

## Changes

<!-- Bullet list of the user-visible / behavioural changes. Keep it short; the diff is the detail. -->

-

## Testing

<!-- How did you verify this? Which conformance tests cover it? Paste command and result. -->

- [ ] `make test` passes locally
- [ ] `make fmt-check` is clean
- [ ] `make clippy` is clean
- [ ] New behaviour has new tests

## Checklist

- [ ] My change references the governing spec section in the commit message(s).
- [ ] I read that spec section before writing the code.
- [ ] I reused existing libraries per Appendix C rather than reimplementing them.
- [ ] Tests cover the new behaviour and pass locally.
- [ ] `make ci` runs all gates clean.
- [ ] Any deferred decision is marked `TODO(open-question-13.x)` with a linked issue.
- [ ] No secrets, credentials, or real PHI are included — synthetic data only.
- [ ] My commits are signed off (DCO — `git commit -s`).
- [ ] My branch is rebased on the latest `main` and has linear history.
- [ ] PR is scoped to one milestone slice and within the ~400-line soft cap (or a
      tracking issue exists).
- [ ] PR description explains the *why*, links the relevant issue, and notes any
      follow-ups left for later PRs.

## Security and data handling

- [ ] This change does not weaken the security model (UDAP/SPIFFE, signature verification,
      authorization at the responding peer, dual-control).
- [ ] If it touches any of the above, I have flagged it for a second reviewer.

## Follow-ups

<!-- Anything intentionally left out and tracked elsewhere. -->

-
