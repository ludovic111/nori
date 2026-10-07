# nori agent evals

Twelve design jobs (`jobs/*.json`), each run headless through `nori-cli agent` with a real model
and scored by automatic checks on the resulting document: pages and sizes, the words asked
for, type sizes and fonts, WCAG contrast of every text layer (`harness.check`), nothing off the
page or past its edge, bleed, masks and untouched original pixels, the files exported (format and
pixel size), and whether the agent looked at its work before finishing. A job passes when every
check passes. Runs are newest first; `nori-evals --record` adds one (see `src/main.rs` and
docs/AI_CONTROL.md, "Evals"). Run them before each release: a harness change that lowers the
pass rate doesn't ship.

