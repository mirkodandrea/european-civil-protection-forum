# Working on this TTX

**WIP — work in progress.** Il progetto è in fase di definizione: requisiti,
funzionalità e direzione possono ancora cambiare.

Read `CLAUDE.md` and the relevant code before changing behavior. The high-level
capabilities and current open topics are summarized in `docs/stato-attuale.md`
and `docs/todo.md`; implementation details are defined by the code.

Reusable project skills are stored under `.claude/skills/<name>/SKILL.md`. Read
only the skills relevant to the task;
their source-game examples do not override TTX requirements or user instructions.

Keep physical decisions in the headless model, approvals in Exercise, and rendering
read-only. New operational state must restore with snapshots. Use paired seeds and
real commands for comparisons. Inspect visual changes through the invisible snapshot
harness; do not bring a window to the foreground as a side effect of verification.

Coordinate ownership before concurrent edits. Format only the files being changed
(`rustfmt --config skip_children=true ...`); avoid unrelated formatting in this repo.
Do not commit, push or publish without an instruction to do so.
