# Driving nori from agents and scripts

Everything a person does in nori's window is a **command** (`family.verb`, JSON in, JSON out).
The window, the built-in agent, `nori-cli` and `nori-mcp` all run the same commands through one
registry, with the same checks and **one undo history**: whatever an agent does is a step the
person can undo, and a batch of edits is one step. The full list, generated from the registry,
is [COMMANDS.md](COMMANDS.md).

## Four ways in

| | How | Notes |
| --- | --- | --- |
| The Agent panel | ⌘J in the window | lsuite AI (sign in, no setup), Claude Code, Codex, the Anthropic or OpenAI API, Ollama or any OpenAI-compatible server. One card per command; "Revert this run". |
| `nori-mcp` | `claude mcp add nori -- /Applications/nori.app/Contents/MacOS/nori-mcp --live` | `--live` drives the open window; `--file poster.nori` works on a file without it; every command is a tool (`layer.add` → `layer_add`). `nori-cli mcp-config` prints the line for Claude Code, Codex, Cursor and Claude Desktop. |
| `nori-cli` | `nori-cli layer.add name=Sky` | On the running app, or `--file <file>` (a `.nori` saves back to itself; a photo or PSD saves to the `.nori` beside it, never over the original), or `--headless` in memory. `nori-cli convert in.psd out.pdf` converts files. |
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
- Look at what you made: `page.look` and `layer.look` return a PNG path, and agents that can see
  get the picture itself (MCP image content, the built-in agent's vision).
- Several related edits: `doc.batch {commands: [{command, params}…]}` is one undo step and rolls
  back if one fails. Sliders and drags pass `coalesce` so a gesture is one step.

## Permissions

Settings › Agent › Permissions apply to the built-in agent and every MCP client (and to
`nori-cli --agent`): editing the open document is always allowed; opening, saving and
exporting files, changing settings, building plugins and quitting each have a switch.
Signing in, API keys and the permissions themselves stay with the person.

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

Write a plugin: `plugin.guide`, `plugin.toolchain`, `plugin.new`, `plugin.writeSource`,
`plugin.build` (errors come back as `{file, line, message}`), `plugin.publishLocal`, then
`filter.apply filter=plugin:<id>`.

Send the picture to kimchi: `handoff.toKimchi` exports a PNG and imports it into kimchi's open
project, on its timeline.
