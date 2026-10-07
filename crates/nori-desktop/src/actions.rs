//! Keyboard shortcuts and the menu bar. Every action ends in a registry command (or a pure view
//! change such as zoom or a panel).
//!
//! [`SHORTCUTS`] is the one list of keys: it binds them, fills the shortcuts sheet, and gives
//! tooltips and menus their hint in this platform's notation (`⇧⌘Z` on macOS, `Ctrl+Shift+Z`
//! elsewhere). Keys are written with `M-` for the platform's command key.

use gpui::{Action, App, KeyBinding, Menu, MenuItem, OsAction, SharedString, actions};

actions!(
    nori,
    [
        Quit,
        About,
        NewDocument,
        OpenDocument,
        PlaceFile,
        Save,
        SaveAs,
        Export,
        CloseDocument,
        Undo,
        Redo,
        OpenSettings,
        ToggleTheme,
        ToggleAgent,
        ShowShortcuts,
        WhatsNew,
        SetUpNori,
        OpenPlugins,
        ZoomIn,
        ZoomOut,
        ZoomFit,
        ZoomActual,
        SelectAll,
        Deselect,
        DeselectAll,
        InvertSelection,
        NewLayer,
        DuplicateLayer,
        DeleteLayer,
        ClearOrDelete,
        MergeDown,
        GroupLayers,
        Ungroup,
        FlattenImage,
        SwapColors,
        ResetColors,
        BrushSmaller,
        BrushBigger,
        Confirm,
        NextPage,
        PreviousPage,
        AddPage,
        ToolMove,
        ToolSelect,
        ToolLasso,
        ToolWand,
        ToolCrop,
        ToolEyedropper,
        ToolBrush,
        ToolEraser,
        ToolFill,
        ToolGradient,
        ToolPen,
        ToolDirect,
        ToolShape,
        ToolText,
        ToolFrame,
        ToolHand,
        ToolZoom,
        OpenHelp,
        OpenSupport,
    ]
);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// Anywhere in the window (with a modifier).
    App,
    /// Only when no text field has the keys and no dialog is open.
    Editing,
}

pub struct Shortcut {
    pub keys: &'static [&'static str],
    pub label: &'static str,
    pub group: &'static str,
    pub scope: Scope,
    action: fn() -> Box<dyn Action>,
}

impl Shortcut {
    pub fn action(&self) -> Box<dyn Action> {
        (self.action)()
    }

    pub fn hint(&self) -> Option<SharedString> {
        self.keys.first().map(|k| keys_label(k))
    }
}

macro_rules! sc {
    ($keys:expr, $label:expr, $group:expr, $scope:expr, $action:expr) => {
        Shortcut { keys: $keys, label: $label, group: $group, scope: $scope, action: || Box::new($action) }
    };
}

pub static SHORTCUTS: &[Shortcut] = &[
    sc!(&["M-n"], "New document", "File", Scope::App, NewDocument),
    sc!(&["M-o"], "Open", "File", Scope::App, OpenDocument),
    sc!(&["M-shift-p"], "Place a file", "File", Scope::App, PlaceFile),
    sc!(&["M-s"], "Save", "File", Scope::App, Save),
    sc!(&["M-shift-s"], "Save as", "File", Scope::App, SaveAs),
    sc!(&["M-shift-e"], "Export", "File", Scope::App, Export),
    sc!(&["M-w"], "Close document", "File", Scope::App, CloseDocument),
    sc!(&["M-z"], "Undo", "Edit", Scope::App, Undo),
    sc!(&["M-shift-z"], "Redo", "Edit", Scope::App, Redo),
    sc!(&["M-a"], "Select all", "Select", Scope::Editing, SelectAll),
    sc!(&["M-d"], "Deselect", "Select", Scope::App, DeselectAll),
    sc!(&["M-shift-i"], "Invert selection", "Select", Scope::App, InvertSelection),
    sc!(&["escape"], "Cancel, deselect", "Select", Scope::Editing, Deselect),
    sc!(&["enter"], "Finish the path", "Tools", Scope::Editing, Confirm),
    sc!(&["M-shift-n"], "New layer", "Layer", Scope::App, NewLayer),
    sc!(&["M-shift-d"], "Duplicate layer", "Layer", Scope::App, DuplicateLayer),
    sc!(&["backspace", "delete"], "Clear the selection's pixels, or delete the layer", "Layer", Scope::Editing, ClearOrDelete),
    sc!(&["M-e"], "Merge down", "Layer", Scope::App, MergeDown),
    sc!(&["M-g"], "Group layers", "Layer", Scope::App, GroupLayers),
    sc!(&["M-shift-g"], "Ungroup", "Layer", Scope::App, Ungroup),
    sc!(&["M-="], "Zoom in", "View", Scope::App, ZoomIn),
    sc!(&["M--"], "Zoom out", "View", Scope::App, ZoomOut),
    sc!(&["M-0"], "Fit on screen", "View", Scope::App, ZoomFit),
    sc!(&["M-1"], "Actual size", "View", Scope::App, ZoomActual),
    sc!(&["pagedown"], "Next page", "View", Scope::Editing, NextPage),
    sc!(&["pageup"], "Previous page", "View", Scope::Editing, PreviousPage),
    sc!(&["M-shift-t"], "Add a page", "View", Scope::App, AddPage),
    sc!(&["M-j"], "Agent", "Window", Scope::App, ToggleAgent),
    sc!(&["M-,"], "Settings", "Window", Scope::App, OpenSettings),
    sc!(&["M-shift-l"], "Light or dark", "Window", Scope::App, ToggleTheme),
    sc!(&["?"], "Keyboard shortcuts", "Window", Scope::Editing, ShowShortcuts),
    sc!(&["x"], "Swap colours", "Colour", Scope::Editing, SwapColors),
    sc!(&["d"], "Black and white", "Colour", Scope::Editing, ResetColors),
    sc!(&["["], "Smaller brush", "Tools", Scope::Editing, BrushSmaller),
    sc!(&["]"], "Bigger brush", "Tools", Scope::Editing, BrushBigger),
    sc!(&["v"], "Move", "Tools", Scope::Editing, ToolMove),
    sc!(&["m"], "Rectangle and ellipse select", "Tools", Scope::Editing, ToolSelect),
    sc!(&["l"], "Lasso", "Tools", Scope::Editing, ToolLasso),
    sc!(&["w"], "Magic wand", "Tools", Scope::Editing, ToolWand),
    sc!(&["c"], "Crop", "Tools", Scope::Editing, ToolCrop),
    sc!(&["i"], "Eyedropper", "Tools", Scope::Editing, ToolEyedropper),
    sc!(&["b"], "Brush", "Tools", Scope::Editing, ToolBrush),
    sc!(&["e"], "Eraser", "Tools", Scope::Editing, ToolEraser),
    sc!(&["g"], "Paint bucket", "Tools", Scope::Editing, ToolFill),
    sc!(&["shift-g"], "Gradient", "Tools", Scope::Editing, ToolGradient),
    sc!(&["p"], "Pen", "Tools", Scope::Editing, ToolPen),
    sc!(&["a"], "Direct selection", "Tools", Scope::Editing, ToolDirect),
    sc!(&["u"], "Shapes", "Tools", Scope::Editing, ToolShape),
    sc!(&["t"], "Type", "Tools", Scope::Editing, ToolText),
    sc!(&["shift-t"], "Text frame", "Tools", Scope::Editing, ToolFrame),
    sc!(&["h"], "Hand", "Tools", Scope::Editing, ToolHand),
    sc!(&["z"], "Zoom", "Tools", Scope::Editing, ToolZoom),
];

/// `M-` → this platform's command key.
fn concrete(k: &str) -> String {
    let m = if cfg!(target_os = "macos") { "cmd-" } else { "ctrl-" };
    k.replace("M-", m)
}

/// Binds every shortcut.
pub fn bind(cx: &mut App) {
    let mut b: Vec<KeyBinding> = crate::ui::input::bindings();
    for s in SHORTCUTS {
        for k in s.keys {
            let context = match s.scope {
                _ if s.action().name() == Quit.name() => None,
                Scope::App => Some("Workspace"),
                Scope::Editing => Some("Workspace && !TextInput && !Modal"),
            };
            match KeyBinding::load(&concrete(k), s.action(), context.map(|c| gpui::KeyBindingContextPredicate::parse(c).expect("valid context").into()), false, None, &gpui::DummyKeyboardMapper) {
                Ok(binding) => b.push(binding),
                Err(e) => tracing::warn!("the key {k} of {} doesn't parse: {e}", s.label),
            }
        }
    }
    // Escape closes a dialog too (Deselect closes what is open first).
    b.push(KeyBinding::new("escape", Deselect, Some("Workspace && Modal")));
    b.push(KeyBinding::new(if cfg!(target_os = "macos") { "cmd-q" } else { "ctrl-q" }, Quit, None));
    cx.bind_keys(b);
}

/// The shortcut of an action, as this platform writes it.
pub fn hint(action: &dyn Action) -> Option<SharedString> {
    SHORTCUTS.iter().find(|s| s.action().name() == action.name()).and_then(Shortcut::hint)
}

/// `"Undo"` → `"Undo (⌘Z)"`.
pub fn tip(label: &str, action: &dyn Action) -> SharedString {
    match hint(action) {
        Some(h) => format!("{label} ({h})").into(),
        None => label.to_string().into(),
    }
}

/// A table keystroke in this platform's notation.
pub fn keys_label(keys: &str) -> SharedString {
    label_for(keys, cfg!(target_os = "macos")).into()
}

pub fn label_for(keys: &str, mac: bool) -> String {
    // `M--` is ⌘ and the minus key.
    let (mods, key): (Vec<&str>, &str) = if keys.len() > 1 && keys.ends_with("--") {
        (keys[..keys.len() - 2].split('-').filter(|m| !m.is_empty()).collect(), "-")
    } else if keys.len() > 1 && keys.contains('-') {
        let mut p: Vec<&str> = keys.split('-').collect();
        let k = p.pop().unwrap_or(keys);
        (p, k)
    } else {
        (vec![], keys)
    };
    let key_name = match key {
        "escape" => if mac { "⎋" } else { "Esc" }.to_string(),
        "enter" => if mac { "↩" } else { "Enter" }.to_string(),
        "backspace" => if mac { "⌫" } else { "Backspace" }.to_string(),
        "delete" => if mac { "⌦" } else { "Delete" }.to_string(),
        "pagedown" => "Page Down".into(),
        "pageup" => "Page Up".into(),
        "" => "-".into(),
        k => k.to_uppercase(),
    };
    let mut out = String::new();
    for m in &mods {
        out.push_str(match (*m, mac) {
            ("M", true) => "⌘",
            ("M", false) => "Ctrl+",
            ("shift", true) => "⇧",
            ("shift", false) => "Shift+",
            ("alt", true) => "⌥",
            ("alt", false) => "Alt+",
            ("ctrl", true) => "⌃",
            ("ctrl", false) => "Ctrl+",
            _ => "",
        });
    }
    out.push_str(&key_name);
    out
}

pub fn menus() -> Vec<Menu> {
    vec![
        Menu::new("nori").items([
            MenuItem::action("About nori", About),
            MenuItem::action("What's New", WhatsNew),
            MenuItem::separator(),
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::action("Plugins…", OpenPlugins),
            MenuItem::separator(),
            MenuItem::action("Quit nori", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New…", NewDocument),
            MenuItem::action("Open…", OpenDocument),
            MenuItem::action("Place…", PlaceFile),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
            MenuItem::action("Save As…", SaveAs),
            MenuItem::action("Export…", Export),
            MenuItem::separator(),
            MenuItem::action("Close", CloseDocument),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            MenuItem::action("Deselect", DeselectAll),
            MenuItem::action("Invert Selection", InvertSelection),
        ]),
        Menu::new("Layer").items([
            MenuItem::action("New Layer", NewLayer),
            MenuItem::action("Duplicate Layer", DuplicateLayer),
            MenuItem::action("Delete Layer", DeleteLayer),
            MenuItem::separator(),
            MenuItem::action("Group", GroupLayers),
            MenuItem::action("Ungroup", Ungroup),
            MenuItem::action("Merge Down", MergeDown),
            MenuItem::action("Flatten Image", FlattenImage),
        ]),
        Menu::new("View").items([
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
            MenuItem::action("Fit on Screen", ZoomFit),
            MenuItem::action("Actual Size", ZoomActual),
            MenuItem::separator(),
            MenuItem::action("Add Page", AddPage),
            MenuItem::separator(),
            MenuItem::action("Light or Dark", ToggleTheme),
        ]),
        Menu::new("Window").items([MenuItem::action("Agent", ToggleAgent), MenuItem::action("Keyboard Shortcuts", ShowShortcuts), MenuItem::action("Set Up nori…", SetUpNori)]),
        Menu::new("Help").items([MenuItem::action("nori Help", OpenHelp), MenuItem::action("Support nori", OpenSupport)]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_platform() {
        assert_eq!(label_for("M-shift-z", true), "⌘⇧Z");
        assert_eq!(label_for("M-shift-z", false), "Ctrl+Shift+Z");
        assert_eq!(label_for("b", false), "B");
        assert_eq!(label_for("M--", false), "Ctrl+-");
        assert_eq!(label_for("escape", true), "⎋");
    }

    #[test]
    fn every_key_is_bound_once_per_scope() {
        let mut seen = std::collections::HashSet::new();
        for s in SHORTCUTS {
            for k in s.keys {
                assert!(seen.insert(*k), "{k} is used twice");
            }
        }
    }
}
