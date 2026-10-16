## What this does

<!-- What the code does now, in a few sentences. Only what is in the diff. -->

## Acceptance criteria

<!-- Each criterion from the issue, and the test or recorded result that meets it. -->

- [ ] ...

<!-- For a port: Spec: <IDs>, upstream <owner>/<repo>@<commit>, and the porting checklist in docs/porting.md. -->

Closes #

## Definition of done

<!-- From plan/backlog/_meta.yaml; plan/check.py keeps this list in step with it. Tick each item, or say why it does not apply. -->

- [ ] The deliverable exists at the path(s) named in the issue and is merged to main
- [ ] Every acceptance criterion is met and each one is checked by a test or a recorded result linked in the issue
- [ ] The verification steps run in CI (cargo xtask ci) or, for manual checks, their output is attached to the issue
- [ ] Public items have rustdoc; a crate README states purpose, layer and spec sections it implements
- [ ] Ported code keeps id's copyright notice and cites the file and function it came from; the behaviour is in docs/spec/ and tests cite it
- [ ] No new clippy allow, unwrap, expect or panic in shipped crates
- [ ] Follow-up work found while doing the issue is filed as new issues and linked, not left in comments
