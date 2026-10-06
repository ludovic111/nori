//! MCP prompts: ready starts for common jobs, written against the registry's tools.

use serde_json::Value;

pub struct Prompt {
    pub name: &'static str,
    pub description: &'static str,
    /// name, description, required
    pub arguments: &'static [(&'static str, &'static str, bool)],
    pub render: fn(&Value) -> String,
}

/// A string argument, or `default` when it is missing or blank.
pub fn arg<'a>(arguments: &'a Value, key: &str, default: &'a str) -> &'a str {
    arguments.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(default)
}

pub const PROMPTS: [Prompt; 4] = [
    Prompt {
        name: "retouch-photo",
        description: "Retouch a photo: tone, colour, crop and sharpen, non-destructively where it can, then export.",
        arguments: &[("photo", "The photo's path (omit to use the open document)", false), ("look", "The look wanted, e.g. \"warm and bright\" or \"moody black and white\"", false)],
        render: |a| {
            let open = match arg(a, "photo", "") {
                "" => "Use the open document (doc_overview).".to_string(),
                p => format!("Open {p} with doc_open."),
            };
            format!(
                "Retouch this photo for a {look} look. {open} Look at it first with page_look. Work non-destructively: layer_addAdjustment for levels or curves \
                 (contrast and tone), hueSaturation or vibrance (colour), exposure; mask an adjustment to part of the picture with select_* then layer_addMask from=selection. \
                 Crop with doc_crop if the framing improves. Sharpen at the end on a copy of the photo layer (layer_duplicate, filter_apply sharpen). Check with page_look after \
                 each step, then export_file a JPEG beside the original and say what you changed.",
                look = arg(a, "look", "natural, balanced"),
            )
        },
    },
    Prompt {
        name: "poster",
        description: "Lay out a poster: a size, a headline, an image or shapes, details, in a clear hierarchy, then PDF and PNG.",
        arguments: &[("brief", "What it announces, e.g. \"jazz night, Friday 8 pm, the Blue Room\"", true), ("size", "A preset (a4, a3, poster-a2, instagram, story) or WxH (default a3)", false)],
        render: |a| {
            format!(
                "Make a poster for: {brief}. Size: {size} (doc_new preset=… or width/height; print presets are 300 dpi). Plan a grid first: page_update margins and columns. \
                 Use one strong headline (text_add, large, tight lineHeight), a supporting line, and the details smaller; define paragraph styles (text_defineStyle) so sizes \
                 stay consistent. Add shapes or a placed image (layer_place) for the visual; vector_addShape fills can be gradients. Align with layer_align relativeTo=margins. \
                 Look with page_look, fix spacing and contrast, then export_file a PDF (vectors and text stay sharp) and a PNG preview.",
                brief = arg(a, "brief", "an event"),
                size = arg(a, "size", "a3"),
            )
        },
    },
    Prompt {
        name: "booklet",
        description: "Set a multi-page booklet: master page with page numbers, text flowing across frames on every page, styles, PDF.",
        arguments: &[("text", "The words, or a path to a text file", true), ("pages", "How many pages (default 8)", false)],
        render: |a| {
            format!(
                "Set this as a booklet of {pages} pages (doc_new preset=a5 pages={pages} margins=…): {text}\n\
                 Make a master page (page_addMaster) with a page number ({{page}} in a small text frame) and apply it to the pages (page_update master=…). Define paragraph \
                 styles for body, headings and captions (text_defineStyle). Put a text frame inside the margins on each page and thread them in order (text_thread from=… to=…) \
                 so the words flow; doc_overview reports text that still overflows. Check pages with page_look, then export_file a PDF.",
                pages = arg(a, "pages", "8"),
                text = arg(a, "text", "(ask for the text)"),
            )
        },
    },
    Prompt {
        name: "write-plugin",
        description: "Write, build and install a nori filter plugin in Rust, then try it.",
        arguments: &[("idea", "What the filter does, e.g. \"a film grain that is stronger in the shadows\"", true)],
        render: |a| {
            format!(
                "Write a nori plugin: {idea}. Follow the recipe: plugin_guide, plugin_toolchain (if Rust is missing, say so and stop), plugin_new, write src/lib.rs with \
                 plugin_writeSource, plugin_build until it is green (fix the errors it lists), plugin_publishLocal. Then try it on the open picture: filter_apply \
                 filter=plugin:<id> and look with page_look.",
                idea = arg(a, "idea", "a filter"),
            )
        },
    },
];
