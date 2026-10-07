# nori agent evals

Twelve design jobs (`jobs/*.json`), each run headless through `nori-cli agent` with a real model
and scored by automatic checks on the resulting document: pages and sizes, the words asked
for, type sizes and fonts, WCAG contrast of every text layer (`harness.check`), nothing off the
page or past its edge, bleed, masks and untouched original pixels, the files exported (format and
pixel size), and whether the agent looked at its work before finishing. A job passes when every
check passes. Runs are newest first; `nori-evals --record` adds one (see `src/main.rs` and
docs/AI_CONTROL.md, "Evals"). Run them before each release: a harness change that lowers the
pass rate doesn't ship.

## 2026-10-08 00:22 · nori 0.2.0 · claude-code with `sonnet`

2/2 jobs passed (100 %), 8/8 checks (100 %). Run: `2026-10-08-0019-claude-code-sonnet`.

| Job | Result | Checks | Commands | Skill loaded | Looked | Time | Failed checks |
| --- | --- | --- | --- | --- | --- | --- | --- |
| batch-headings | pass | 3/3 | 6 | — | yes | 30 s | — |
| contrast-fix | pass | 5/5 | 7 | — | yes | 27 s | — |

Two cheap jobs for 0.2 (the other ten are written and scored the same way; run them all before
the next release). Both agents looked at their work and re-checked it before finishing. In
contrast-fix the first `doc.batch` failed because the agent named a command by its tool name
(`text_update`); `doc.batch` now takes either form.

