# Driving nori from agents and scripts

Everything a person does in nori's window is a **command** (`family.verb`, JSON in, JSON out).
The window, the built-in agent, `nori-cli` and `nori-mcp` all run the same commands through one
registry, with the same checks and **one undo history**: whatever an agent does is a step the
person can undo, and a batch of edits is one step. The full list, generated from the registry,
is [COMMANDS.md](COMMANDS.md).

## Four ways in

| | How | Notes |
| --- | --- | --- |
| The Agent panel | ⌘J in the window | Claude Code (the default), Codex, the Anthropic or OpenAI API, Ollama or any OpenAI-compatible server. One card per command; "Revert this run". |
| `nori-mcp` | `claude mcp add nori -- /Applications/nori.app/Contents/MacOS/nori-mcp --live` | `--live` drives the open window; `--file poster.nori` works on a file without it; every command is a tool (`layer.add` → `layer_add`). `nori-cli mcp-config` prints the line for Claude Code, Codex, Cursor and Claude Desktop. |
| `nori-cli` | `nori-cli layer.add name=Sky` | On the running app, or `--file <file>` (a `.nori` saves back to itself; a photo or PSD saves to the `.nori` beside it, never over the original), or `--headless` in memory. `nori-cli convert in.psd out.pdf` converts files. `nori-cli --file poster.nori agent "…" --provider claude-code` runs the built-in agent on a file without the app (`--model`, `--max-steps`, `--json`). |
| The bridge | `127.0.0.1`, token in `control.json` (0600) in nori's data folder | Newline-delimited JSON-RPC 2.0: `auth {token, client}` first, then any command. The same protocol as kimchi's and ryolune's. |

## Conventions

- Positions and sizes are **document pixels on the page**: x right, y down, from the page's
  top-left corner. `doc.overview` gives each page's size; `dpi` says how big a pixel is in print.
- Layers are named by id (`L12`) or by a unique name (`"Sky"`); pages by id, name or number
  from 1. A near miss answers with "did you mean".
- Colours are `#rrggbb` or `#rrggbbaa`. Fills and strokes take a colour, `"none"`, or a gradient
  `{"type": "linear", "x1", "y1", "x2", "y2", "stops": ["#000", "#fff"]}`.
- Read `doc.overview` first: pages, every layer as a tree (with bounds, text and shapes),
  styles, colours, history and problems (text that overflows its frame).
- Look at what you made: `harness.look`, `page.look` and `layer.look` return a PNG path, and
  agents that can see get the picture itself (MCP image content, the built-in agent's vision);
  `harness.look` and `harness.check` add the numbers (contrast, bounds, resolution, overflow).
- Several related edits: `doc.batch {commands: [{command, params}…]}` is one undo step and rolls
  back if one fails. Sliders and drags pass `coalesce` so a gesture is one step.

## The harness: what makes an agent good here

Every agent in nori (the Agent panel's, Claude Code or Codex through `nori-mcp`, the lsuite app's)
gets the same harness (lsuite's HARNESS.md), from `crates/nori-control/src/harness/`:

| Part | Where |
| --- | --- |
| The expert brief: the trade's quality bar (retouching, compositing, vectors, type and grids, print, colour, contrast, accessibility), the document's model, the commands for the common jobs, the usual mistakes, the finish routine | `harness.brief`; it *is* the Agent panel's system prompt and `nori-mcp`'s instructions (`harness/brief.md`, one source) |
| Skills: eleven playbooks (retouch-photo, cutout-composite, poster, social-set, logo, booklet, brand-kit, mockup, export-print-web, batch-edits, write-plugin) with the exact commands and the checks that prove them | `harness.skills`, `harness.skill name=poster`; over MCP each is a prompt and a resource `nori://skills/<name>` (`harness/skills/*.md`) |
| Live context: the page and its grid, its layers, the active layer, the selection, quick problems, and what others changed since the agent's last step | `harness.context since=<seq>`; the Agent panel refreshes it before every model step, `nori-mcp` adds it to a tool result when the document changed, after the text and in `structuredContent.harnessNotes` with the other notes (file saved, the finish routine after a change), since Claude Code shows only the structured content when there is some (`NORI_MCP_CONTEXT=0` turns the context off), resource `nori://harness/context` |
| Eyes and numbers | `harness.look` (the page as a picture the model sees, with the checks) and `harness.check` (the checks alone, `all=true` for every page): WCAG contrast of each text layer against what is behind it, things off the page, past its edge or margins, short of the bleed, pictures' real detail per inch at print size, overflowing text, empty layers, colours CMYK can't print |
| The finish routine: look, compare with the request, fix (up to three passes), report | in the brief and every skill; the evals check it |
| One undo per agent turn | a checkpoint before the run's first edit; "Revert this run" / `agent.revert` |

## Evals

`evals/` holds twelve design jobs (poster, retouch, cut-out, social set, logo, booklet, brand
kit, mockup, print and web export, batch restyle, contrast fix, business card), each a fixture,
a request in a designer's words and automatic checks on the resulting document. They run
headless through `nori-cli agent` with a real model; the Claude Code provider needs no key:

```sh
cargo build -p nori-cli -p nori-mcp -p nori-evals
target/debug/nori-evals --model sonnet --only poster,logo --record   # or --provider anthropic
```

Scores go to `evals/RESULTS.md` (date, model, pass rate, per job). Run them before each release:
a harness change that lowers the pass rate doesn't ship.

## Permissions

Settings › Agent › Permissions apply to the built-in agent and every MCP client (and to
`nori-cli --agent`): editing the open document is always allowed; opening, saving and
exporting files, changing settings, building plugins and quitting each have a switch.
Choosing the agent's provider, API keys and the permissions themselves stay with the person.

## Recipes

Retouch a photo:

```sh
nori-cli --file photo.jpg layer.addAdjustment kind=curves --args '{"settings":{"rgb":[[0,0],[64,52],[192,206],[255,255]]}}'
nori-cli --file photo.nori layer.addAdjustment kind=vibrance --args '{"settings":{"vibrance":30}}'
nori-cli --file photo.nori export.file path=photo-edit.jpg quality=88
```

Lay out a poster:

```sh
nori-cli --file poster.nori doc.new preset=a3 margins=150
nori-cli --file poster.nori text.add text="SUMMER NIGHTS" x=150 y=300 size=300 weight=800
nori-cli --file poster.nori vector.addShape shape=rect x=150 y=1200 width=3200 height=1800 radius=40 --args '{"fill":{"type":"linear","x1":150,"y1":1200,"x2":3350,"y2":3000,"stops":["#ff5f6d","#ffc371"]}}'
nori-cli --file poster.nori export.file path=poster.pdf
```

A booklet whose text flows from page to page: `doc.new preset=a5 pages=8`, a frame on each page
(`text.add … frameWidth frameHeight`), then `text.thread from=L3 to=L5` for each pair; a master
page (`page.addMaster`) with `{page}` in a small frame numbers every page.

Prepare for print: `page.update bleed=35` (3 mm at 300 dpi) on pages whose artwork reaches the
edge, `harness.check` (no errors), then `export.file path=print.pdf`: the PDF's sheet includes the
bleed and marks the trim.

Write a plugin: `plugin.guide`, `plugin.toolchain`, `plugin.new`, `plugin.writeSource`,
`plugin.build` (errors come back as `{file, line, message}`), `plugin.publishLocal`, then
`filter.apply filter=plugin:<id>`.

Send the picture to kimchi: `handoff.toKimchi` exports a PNG and imports it into kimchi's open
project, on its timeline.

## Updates

`app.checkUpdates` reads `<server>/api/apps/nori/latest.json` with no account and no
`Authorization` header (`<server>` is `LSUITE_SERVER`, else lsuite.xyz); the files it lists are
on the server's public file route. `NORI_UPDATE_URL` points it at another `latest.json` (tests).
Builds are published with `scripts/publish-build.sh <version> <run-id>` (from a run of kimchi's
suite build) to the private `ludovic111/lsuite-builds`.
