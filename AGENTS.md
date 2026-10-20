# Working on this TTX

Read `CLAUDE.md` and the relevant documentation before changing behavior.
`docs/lessons-from-propagator-abm.md` records the comparison; current implementation
and verification are in `docs/operational-upgrade.md`.

Reusable project skills are indexed in `docs/ttx-skills.md` and stored under
`.claude/skills/<name>/SKILL.md`. Read only the skills relevant to the task;
their source-game examples do not override TTX requirements or user instructions.

Keep physical decisions in the headless model, approvals in Exercise, and rendering
read-only. New operational state must restore with snapshots. Use paired seeds and
real commands for comparisons. Inspect visual changes through the invisible snapshot
harness; do not bring a window to the foreground as a side effect of verification.

Coordinate ownership before concurrent edits. Format only the files being changed
(`rustfmt --config skip_children=true ...`); avoid unrelated formatting in this repo.
Do not commit, push or publish without an instruction to do so.
