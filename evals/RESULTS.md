# nori agent evals

Twelve design jobs (`jobs/*.json`), each run headless through `nori-cli agent` with a real model
and scored by automatic checks on the resulting document: pages and sizes, the words asked
for, type sizes and fonts, WCAG contrast of every text layer (`harness.check`), nothing off the
page or past its edge, bleed, masks and untouched original pixels, the files exported (format and
pixel size), and whether the agent looked at its work before finishing. A job passes when every
check passes. Runs are newest first; `nori-evals --record` adds one (see `src/main.rs` and
docs/AI_CONTROL.md, "Evals"). Run them before each release: a harness change that lowers the
pass rate doesn't ship.

## 2026-10-08 12:26 · nori 0.2.0 · claude-code with `opus`

12/12 jobs passed (100 %), 82/82 checks (100 %). Run: `2026-10-08-1212-claude-code-opus`.

| Job | Result | Checks | Commands | Skill loaded | Looked | Time | Failed checks |
| --- | --- | --- | --- | --- | --- | --- | --- |
| poster | pass | 11/11 | 8 | poster | yes | 168 s | — |
| retouch | pass | 5/5 | 16 | retouch-photo | yes | 88 s | — |
| cutout | pass | 6/6 | 14 | cutout-composite | yes | 115 s | — |
| social-set | pass | 11/11 | 20 | social-set | yes | 156 s | — |
| logo | pass | 7/7 | 12 | logo | yes | 50 s | — |
| booklet | pass | 8/8 | 45 | booklet | yes | 337 s | — |
| brand-kit | pass | 8/8 | 12 | brand-kit | yes | 137 s | — |
| mockup | pass | 6/6 | 8 | mockup | yes | 74 s | — |
| print-web | pass | 5/5 | 6 | export-print-web | yes | 52 s | — |
| batch-headings | pass | 3/3 | 6 | batch-edits | yes | 25 s | — |
| contrast-fix | pass | 5/5 | 3 | — | yes | 30 s | — |
| business-card | pass | 7/7 | 8 | poster | yes | 42 s | — |

The full set on the release model for 0.2.0, run 12:12–12:26. As first scored it was 9/12 (79/82
checks), and all three failures came from the scorer, not the agent:
- poster and booklet set their titles on two lines ("Blue Hour\nJazz", "A Short Guide\nto
  Tea"), and the `text` check didn't treat a line break as a space.
- logo built its cloud as one compound vector (`vector_combine`, as the brief says) beside a text
  wordmark, and the job wanted two vector layers.

The `text` check now matches across line breaks, and the logo job wants one vector layer or
more. The same documents were then rescored (`--score-only`) to the result above. Every agent
loaded its skill (except contrast-fix, a one-line fix) and looked before it finished.

## 2026-10-08 02:02 · nori 0.2.0 · claude-code with `sonnet`

1/1 jobs passed (100 %), 5/5 checks (100 %). Run: `2026-10-08-0202-claude-code-sonnet`.

| Job | Result | Checks | Commands | Skill loaded | Looked | Time | Failed checks |
| --- | --- | --- | --- | --- | --- | --- | --- |
| contrast-fix | pass | 5/5 | 4 | — | yes | 20 s | — |

After notes went into `structuredContent.harnessNotes` (Claude Code shows only the structured
content of a result that has both) and `doc.batch` learnt tool names: the batch went through
first time, and the agent looked again after its edit.

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

