## Description

<!-- What is broken, in one or two sentences: the observable wrong behavior
     and the expected behavior. Ground both in reality — the exact response,
     exit code, or state observed, not a paraphrase of it. -->

## Steps to reproduce

<!-- Numbered, exact, copy-pasteable. Commands with real arguments, the
     environment they ran in (dev stack, tenant, seed), and the observed
     output at the failing step. If it is intermittent, say what makes it
     rare and what the failure signature is. -->

1.

## Root cause

<!-- The mechanism, not the symptom: which line/layer produced the wrong
     result and why, with path:line pointers into the code as it stands.
     If the cause is not yet known, mark this section HYPOTHESIS and make
     proving it the first criterion below. -->

## Fix

<!-- The change and where it lands. Say explicitly when the fix is at the
     seam rather than the symptom — patching the caller instead of the
     helper, fixing the generator instead of the generated file. -->

## Guard

<!-- How this class of bug is prevented from coming back: the regression
     test that pins the behavior, or the lint/gate/check that makes the
     same mistake fail loudly next time. A fix without a guard will be
     re-shipped broken. -->

## Acceptance criteria

- [ ] The reproduction steps above now produce the expected output.
- [ ] <!-- the regression test, named and running in CI -->

## Do not

<!-- Repairs this bug deliberately does not attempt (refactors, drive-by
     cleanups, related-but-distinct defects) and where they go instead. -->
