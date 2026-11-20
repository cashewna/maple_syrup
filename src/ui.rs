use crate::plot::PlotData;
use crate::worksheet::{CellOutput, MatrixPresetKind, MatrixTemplate, Worksheet};
use gpui::{
    App, Bounds, ClipboardItem, Context, FocusHandle, Focusable, FontWeight, Hsla, KeyBinding,
    KeyDownEvent, PathBuilder, Pixels, Rgba, ScrollHandle, Window, actions, canvas, div, fill,
    point, prelude::*, px, rgb, size,
};

actions!(
    maple_worksheet,
    [
        Evaluate,
        ExecuteWorksheet,
        ToggleMathTextMode,
        Backspace,
        Delete,
        MoveLeft,
        MoveRight,
        MoveHome,
        MoveEnd,
        HistoryPrev,
        HistoryNext,
        ClearWorksheet,
        RestartKernel,
        NewWorksheetTab,
        Paste,
        CopyLastOutput,
        Quit,
    ]
);

/// Register keyboard shortcuts matching the Maple worksheet reference keymap.
pub fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Evaluate, None),
        KeyBinding::new("ctrl-shift-enter", ExecuteWorksheet, None),
        KeyBinding::new("f5", ToggleMathTextMode, None),
        KeyBinding::new("backspace", Backspace, None),
        KeyBinding::new("delete", Delete, None),
        KeyBinding::new("left", MoveLeft, None),
        KeyBinding::new("right", MoveRight, None),
        KeyBinding::new("home", MoveHome, None),
        KeyBinding::new("end", MoveEnd, None),
        KeyBinding::new("up", HistoryPrev, None),
        KeyBinding::new("down", HistoryNext, None),
        KeyBinding::new("ctrl-l", ClearWorksheet, None),
        KeyBinding::new("ctrl-r", RestartKernel, None),
        KeyBinding::new("ctrl-n", NewWorksheetTab, None),
        KeyBinding::new("ctrl-v", Paste, None),
        KeyBinding::new("ctrl-shift-c", CopyLastOutput, None),
        KeyBinding::new("ctrl-q", Quit, None),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeftDockTab {
    Palettes,
    Workbook,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryMode {
    Math,
    Text,
}

enum DocumentContent {
    StartPage,
    Worksheet(Box<Worksheet>),
}

struct WorksheetTab {
    id: usize,
    title: String,
    content: DocumentContent,
    selected_cell: Option<usize>,
    input: String,
    cursor: usize,
    history: Vec<String>,
    history_pos: Option<usize>,
    last_elapsed_us: Option<u128>,
    scroll_handle: ScrollHandle,
}

impl WorksheetTab {
    fn start_page(id: usize) -> Self {
        Self {
            id,
            title: "Start.mw".to_string(),
            content: DocumentContent::StartPage,
            selected_cell: None,
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            last_elapsed_us: None,
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn new_worksheet(id: usize, title: String, preset: Option<&str>) -> Self {
        let mut ws = Worksheet::new();
        let mut history = Vec::new();
        let mut last_elapsed_us = None;
        if let Some(p) = preset {
            let added = ws.load_preset(p);
            last_elapsed_us = added.last().map(|c| c.elapsed_us);
            for cell in &added {
                history.push(cell.input.clone());
            }
        }
        let selected_cell = ws.cells.len().checked_sub(1);
        Self {
            id,
            title,
            content: DocumentContent::Worksheet(Box::new(ws)),
            selected_cell,
            input: String::new(),
            cursor: 0,
            history,
            history_pos: None,
            last_elapsed_us,
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn worksheet(&self) -> Option<&Worksheet> {
        match &self.content {
            DocumentContent::Worksheet(ws) => Some(ws),
            DocumentContent::StartPage => None,
        }
    }

    fn worksheet_mut(&mut self) -> Option<&mut Worksheet> {
        match &mut self.content {
            DocumentContent::Worksheet(ws) => Some(ws),
            DocumentContent::StartPage => None,
        }
    }
}

struct PaletteExpansion {
    expression: bool,
    calculus: bool,
    variables: bool,
    matrix: bool,
    symbols: bool,
}

impl Default for PaletteExpansion {
    fn default() -> Self {
        Self {
            expression: true,
            calculus: true,
            variables: true,
            matrix: true,
            symbols: false,
        }
    }
}

/// Interactive Maple-inspired CAS workspace rendered with GPUI.
pub struct MapleWorkspaceView {
    tabs: Vec<WorksheetTab>,
    active_tab: usize,
    next_tab_id: usize,
    next_worksheet_num: usize,
    left_dock_tab: LeftDockTab,
    palettes: PaletteExpansion,
    matrix_rows: usize,
    matrix_cols: usize,
    matrix_kind: MatrixPresetKind,
    right_dock_open: bool,
    entry_mode: EntryMode,
    focus_handle: FocusHandle,
    status_message: String,
}

impl Focusable for MapleWorkspaceView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MapleWorkspaceView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let start = WorksheetTab::start_page(1);
        let initial_ws = WorksheetTab::new_worksheet(2, "Worksheet 1.mw".to_string(), None);
        Self {
            tabs: vec![start, initial_ws],
            active_tab: 0,
            next_tab_id: 3,
            next_worksheet_num: 2,
            left_dock_tab: LeftDockTab::Palettes,
            palettes: PaletteExpansion::default(),
            matrix_rows: 2,
            matrix_cols: 2,
            matrix_kind: MatrixPresetKind::Identity,
            right_dock_open: true,
            entry_mode: EntryMode::Math,
            focus_handle: cx.focus_handle(),
            status_message: "Ready".to_string(),
        }
    }

    fn current_tab(&self) -> &WorksheetTab {
        &self.tabs[self.active_tab]
    }

    fn current_tab_mut(&mut self) -> &mut WorksheetTab {
        &mut self.tabs[self.active_tab]
    }

    fn ensure_worksheet_tab(&mut self) -> usize {
        if self.current_tab().worksheet().is_some() {
            return self.active_tab;
        }
        if let Some(pos) = self.tabs.iter().position(|t| t.worksheet().is_some()) {
            self.active_tab = pos;
            return pos;
        }
        self.open_new_worksheet_tab(None)
    }

    fn open_new_worksheet_tab(&mut self, preset: Option<&str>) -> usize {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let num = self.next_worksheet_num;
        self.next_worksheet_num += 1;
        let title = match preset {
            Some("calculus") => format!("Calculus {num}.mw"),
            Some("linalg") => format!("LinearAlgebra {num}.mw"),
            Some("series") => format!("Series {num}.mw"),
            Some("plot") => format!("Plot {num}.mw"),
            _ => format!("Worksheet {num}.mw"),
        };
        let tab = WorksheetTab::new_worksheet(id, title, preset);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.active_tab
    }

    fn close_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx == 0 || idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > idx {
            self.active_tab -= 1;
        }
        cx.notify();
    }

    fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        self.ensure_worksheet_tab();
        let clean = text.replace(['\r', '\n'], " ");
        let tab = self.current_tab_mut();
        tab.input.insert_str(tab.cursor, &clean);
        tab.cursor += clean.len();
        tab.history_pos = None;
        cx.notify();
    }

    fn set_input_and_focus(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_worksheet_tab();
        let tab = self.current_tab_mut();
        tab.input = text.to_string();
        tab.cursor = tab.input.len();
        tab.history_pos = None;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn evaluate_input(&mut self, _: &Evaluate, _window: &mut Window, cx: &mut Context<Self>) {
        if self.current_tab().worksheet().is_none() {
            self.ensure_worksheet_tab();
            cx.notify();
            return;
        }

        let trimmed = self.current_tab().input.trim().to_string();
        if trimmed.is_empty() {
            // If prompt is empty and a cell is selected, re-evaluate the selected cell.
            if let Some(sel_idx) = self.current_tab().selected_cell
                && let Some(ws) = self.current_tab().worksheet()
                && let Some(cell) = ws.cells.get(sel_idx)
            {
                let input_copy = cell.input.clone();
                self.run_statement_in_active_tab(&input_copy, false, cx);
            }
            return;
        }

        if self.entry_mode == EntryMode::Text {
            self.status_message = "Text mode active — press F5 to switch to Math mode".to_string();
            cx.notify();
            return;
        }

        self.run_statement_in_active_tab(&trimmed, true, cx);
    }

    fn run_statement_in_active_tab(
        &mut self,
        stmt: &str,
        clear_prompt: bool,
        cx: &mut Context<Self>,
    ) {
        self.ensure_worksheet_tab();
        let tab = self.current_tab_mut();
        let Some(ws) = tab.worksheet_mut() else {
            return;
        };
        let added = ws.evaluate(stmt);
        let total_us: u128 = added.iter().map(|c| c.elapsed_us).sum();
        let last_elapsed_us = added.last().map(|c| c.elapsed_us);
        let cell_count = ws.cells.len();
        tab.last_elapsed_us = last_elapsed_us;
        tab.selected_cell = cell_count.checked_sub(1);
        if tab.history.last().map(String::as_str) != Some(stmt) {
            tab.history.push(stmt.to_string());
        }
        tab.history_pos = None;
        if clear_prompt {
            tab.input.clear();
            tab.cursor = 0;
        }
        tab.scroll_handle.scroll_to_bottom();
        self.status_message = format!("Evaluated {} statement(s) ({total_us} μs)", added.len());
        cx.notify();
    }

    fn execute_worksheet(
        &mut self,
        _: &ExecuteWorksheet,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ws) = self.current_tab_mut().worksheet_mut() else {
            self.status_message = "Open a worksheet tab before executing worksheet".to_string();
            cx.notify();
            return;
        };
        ws.execute_all();
        let count = ws.cells.len();
        let total_us: u128 = ws.cells.iter().map(|c| c.elapsed_us).sum();
        let last_us = ws.cells.last().map(|c| c.elapsed_us);
        let tab = self.current_tab_mut();
        tab.last_elapsed_us = last_us;
        tab.selected_cell = count.checked_sub(1);
        tab.scroll_handle.scroll_to_bottom();
        self.status_message = format!("Executed worksheet ({count} cells in {total_us} μs)");
        cx.notify();
    }

    fn toggle_math_text_mode(
        &mut self,
        _: &ToggleMathTextMode,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.entry_mode = match self.entry_mode {
            EntryMode::Math => EntryMode::Text,
            EntryMode::Text => EntryMode::Math,
        };
        self.status_message = match self.entry_mode {
            EntryMode::Math => "Switched to Math Mode".to_string(),
            EntryMode::Text => "Switched to Text Mode".to_string(),
        };
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        if tab.cursor == 0 {
            return;
        }
        let prev = prev_char_boundary(&tab.input, tab.cursor);
        tab.input.drain(prev..tab.cursor);
        tab.cursor = prev;
        cx.notify();
    }

    fn delete(&mut self, _: &Delete, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        if tab.cursor >= tab.input.len() {
            return;
        }
        let next = next_char_boundary(&tab.input, tab.cursor);
        tab.input.drain(tab.cursor..next);
        cx.notify();
    }

    fn move_left(&mut self, _: &MoveLeft, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        if tab.cursor > 0 {
            tab.cursor = prev_char_boundary(&tab.input, tab.cursor);
            cx.notify();
        }
    }

    fn move_right(&mut self, _: &MoveRight, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        if tab.cursor < tab.input.len() {
            tab.cursor = next_char_boundary(&tab.input, tab.cursor);
            cx.notify();
        }
    }

    fn move_home(&mut self, _: &MoveHome, _window: &mut Window, cx: &mut Context<Self>) {
        self.current_tab_mut().cursor = 0;
        cx.notify();
    }

    fn move_end(&mut self, _: &MoveEnd, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        tab.cursor = tab.input.len();
        cx.notify();
    }

    fn history_prev(&mut self, _: &HistoryPrev, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        if tab.history.is_empty() {
            return;
        }
        let next_idx = match tab.history_pos {
            None => tab.history.len() - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        tab.history_pos = Some(next_idx);
        tab.input = tab.history[next_idx].clone();
        tab.cursor = tab.input.len();
        cx.notify();
    }

    fn history_next(&mut self, _: &HistoryNext, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        let Some(pos) = tab.history_pos else {
            return;
        };
        if pos + 1 < tab.history.len() {
            let next_idx = pos + 1;
            tab.history_pos = Some(next_idx);
            tab.input = tab.history[next_idx].clone();
            tab.cursor = tab.input.len();
        } else {
            tab.history_pos = None;
            tab.input.clear();
            tab.cursor = 0;
        }
        cx.notify();
    }

    fn clear_worksheet(
        &mut self,
        _: &ClearWorksheet,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ws) = self.current_tab_mut().worksheet_mut() {
            ws.clear_cells();
            self.current_tab_mut().selected_cell = None;
            self.status_message = "Cleared worksheet cells (kernel variables retained)".to_string();
            cx.notify();
        }
    }

    fn restart_kernel(&mut self, _: &RestartKernel, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ws) = self.current_tab_mut().worksheet_mut() {
            ws.restart();
            let tab = self.current_tab_mut();
            tab.last_elapsed_us = None;
            self.status_message = "Kernel restarted — all bindings cleared".to_string();
            cx.notify();
        }
    }

    fn new_worksheet_action(
        &mut self,
        _: &NewWorksheetTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_new_worksheet_tab(None);
        self.status_message = format!("Opened {}", self.current_tab().title);
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn paste(&mut self, _: &Paste, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.insert_text(&text, cx);
        }
    }

    fn copy_last_output(
        &mut self,
        _: &CopyLastOutput,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ws) = self.current_tab().worksheet() else {
            return;
        };
        let target = self
            .current_tab()
            .selected_cell
            .and_then(|idx| ws.cells.get(idx))
            .or_else(|| ws.cells.last());
        let Some(cell) = target else {
            return;
        };
        let text = match &cell.output {
            CellOutput::Text(t) => t.clone(),
            CellOutput::MatrixGrid(rows) => format!(
                "Matrix([{}])",
                rows.iter()
                    .map(|r| format!("[{}]", r.join(", ")))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            CellOutput::Plot(p) => p.title.clone(),
            CellOutput::Suppressed => return,
            CellOutput::Error(e) => e.clone(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.status_message = format!("Copied output (%{}) to clipboard", cell.index);
        cx.notify();
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let mods = &ev.keystroke.modifiers;
        if mods.control || mods.alt || mods.platform {
            return;
        }
        if let Some(ref key_char) = ev.keystroke.key_char
            && !key_char.is_empty()
            && !key_char.chars().all(|c| c.is_control())
        {
            self.insert_text(key_char, cx);
            return;
        }
        if ev.keystroke.key == "space" {
            self.insert_text(" ", cx);
        }
    }

    fn open_starter_worksheet(
        &mut self,
        preset: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_new_worksheet_tab(preset);
        self.status_message = format!("Opened {}", self.current_tab().title);
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn run_context_action(
        &mut self,
        cmd_template: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ws) = self.current_tab().worksheet() else {
            return;
        };
        let sel_idx = self
            .current_tab()
            .selected_cell
            .filter(|&i| i < ws.cells.len())
            .or_else(|| ws.cells.len().checked_sub(1));
        let Some(idx) = sel_idx else {
            return;
        };
        let cmd = if idx + 1 == ws.cells.len() {
            cmd_template.to_string()
        } else {
            let target_expr = match &ws.cells[idx].output {
                CellOutput::Text(t) => format!("({t})"),
                CellOutput::MatrixGrid(rows) => format!(
                    "Matrix([{}])",
                    rows.iter()
                        .map(|r| format!("[{}]", r.join(", ")))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => "%".to_string(),
            };
            cmd_template.replace('%', &target_expr)
        };
        self.run_statement_in_active_tab(&cmd, false, cx);
        window.focus(&self.focus_handle);
    }

    fn cycle_matrix_kind(&mut self, cx: &mut Context<Self>) {
        self.matrix_kind = match self.matrix_kind {
            MatrixPresetKind::Zero => MatrixPresetKind::Identity,
            MatrixPresetKind::Identity => MatrixPresetKind::Sequential,
            MatrixPresetKind::Sequential => MatrixPresetKind::Symbolic,
            MatrixPresetKind::Symbolic => MatrixPresetKind::Zero,
        };
        cx.notify();
    }

    fn insert_matrix_from_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let expr = MatrixTemplate::build_expr(self.matrix_rows, self.matrix_cols, self.matrix_kind);
        self.insert_text(&expr, cx);
        window.focus(&self.focus_handle);
    }

    fn render_menu_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mode_label = match self.entry_mode {
            EntryMode::Math => "Math",
            EntryMode::Text => "Text",
        };
        let menu_items = [
            "File",
            "Home",
            "Insert",
            "Edit",
            "View",
            "Evaluation",
            "Tools",
            "Workbook",
            "Help",
        ];

        div()
            .w_full()
            .h(px(30.))
            .px_3()
            .bg(rgb(0xeaf0f6))
            .border_b_1()
            .border_color(rgb(0xd4dde7))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(div().flex().flex_row().items_center().gap_1().children(
                menu_items.into_iter().enumerate().map(|(idx, label)| {
                    div()
                        .id(("menu", idx))
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .text_xs()
                        .text_color(rgb(0x1e293b))
                        .hover(|s| s.bg(rgb(0xdbe5f0)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            match label {
                                "File" | "Home" => {
                                    this.active_tab = 0;
                                    cx.notify();
                                }
                                "Insert" => {
                                    this.left_dock_tab = LeftDockTab::Palettes;
                                    this.palettes.matrix = true;
                                    cx.notify();
                                }
                                "Evaluation" => {
                                    this.execute_worksheet(&ExecuteWorksheet, window, cx);
                                }
                                "Workbook" => {
                                    this.left_dock_tab = LeftDockTab::Workbook;
                                    cx.notify();
                                }
                                _ => {
                                    this.status_message = format!("{label} menu active");
                                    cx.notify();
                                }
                            }
                            window.focus(&this.focus_handle);
                        }))
                        .child(label)
                }),
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("mode-pill")
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xcbd5e1))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(0x0f172a))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0xf1f5f9)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_math_text_mode(&ToggleMathTextMode, window, cx);
                            }))
                            .child(format!("Entry: {mode_label} (F5)")),
                    )
                    .child(
                        div()
                            .id("toggle-right-dock")
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xcbd5e1))
                            .text_xs()
                            .text_color(rgb(0x334155))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0xf1f5f9)))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.right_dock_open = !this.right_dock_open;
                                cx.notify();
                            }))
                            .child(if self.right_dock_open { ">>" } else { "<<" }),
                    ),
            )
    }

    fn render_ribbon(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .px_3()
            .py_2()
            .bg(rgb(0xf4f7fb))
            .border_b_1()
            .border_color(rgb(0xd4dde7))
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            // Navigation group
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .child(ribbon_btn(
                        "rib-home",
                        "⌂",
                        "Start",
                        rgb(0x1d4ed8),
                        cx.listener(|this, _, window, cx| {
                            this.active_tab = 0;
                            window.focus(&this.focus_handle);
                            cx.notify();
                        }),
                    ))
                    .child(ribbon_btn(
                        "rib-new",
                        "[>",
                        "New Sheet",
                        rgb(0x0f766e),
                        cx.listener(|this, _, window, cx| {
                            this.open_starter_worksheet(None, window, cx);
                        }),
                    )),
            )
            .child(div().w(px(1.)).h(px(28.)).bg(rgb(0xcbd5e1)))
            // Clipboard group
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .child(ribbon_btn(
                        "rib-copy",
                        "⧉",
                        "Copy",
                        rgb(0x334155),
                        cx.listener(|this, _, window, cx| {
                            this.copy_last_output(&CopyLastOutput, window, cx);
                        }),
                    ))
                    .child(ribbon_btn(
                        "rib-paste",
                        "📋",
                        "Paste",
                        rgb(0x334155),
                        cx.listener(|this, _, window, cx| {
                            this.paste(&Paste, window, cx);
                        }),
                    )),
            )
            .child(div().w(px(1.)).h(px(28.)).bg(rgb(0xcbd5e1)))
            // Execution group (mirrors Maple 2025 ribbon)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .child(ribbon_btn(
                        "rib-exec-all",
                        "!!!",
                        "Execute Worksheet",
                        rgb(0x1d4ed8),
                        cx.listener(|this, _, window, cx| {
                            this.execute_worksheet(&ExecuteWorksheet, window, cx);
                        }),
                    ))
                    .child(ribbon_btn(
                        "rib-exec-sel",
                        "!",
                        "Execute Selection",
                        rgb(0x2563eb),
                        cx.listener(|this, _, window, cx| {
                            this.evaluate_input(&Evaluate, window, cx);
                        }),
                    ))
                    .child(ribbon_btn(
                        "rib-exec-code",
                        "|>",
                        "Execute Code",
                        rgb(0x059669),
                        cx.listener(|this, _, window, cx| {
                            this.evaluate_input(&Evaluate, window, cx);
                        }),
                    ))
                    .child(ribbon_btn(
                        "rib-restart",
                        "↻",
                        "Restart Kernel",
                        rgb(0xb91c1c),
                        cx.listener(|this, _, window, cx| {
                            this.restart_kernel(&RestartKernel, window, cx);
                        }),
                    ))
                    .child(ribbon_btn(
                        "rib-clear",
                        "⌫",
                        "Clear Sheet",
                        rgb(0x475569),
                        cx.listener(|this, _, window, cx| {
                            this.clear_worksheet(&ClearWorksheet, window, cx);
                        }),
                    )),
            )
    }

    fn render_left_dock(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(272.))
            .h_full()
            .bg(rgb(0xf8fafc))
            .border_r_1()
            .border_color(rgb(0xd4dde7))
            .flex()
            .flex_col()
            .child(
                // Palettes | Workbook tab bar
                div()
                    .w_full()
                    .h(px(32.))
                    .bg(rgb(0xeaf0f6))
                    .border_b_1()
                    .border_color(rgb(0xd4dde7))
                    .flex()
                    .flex_row()
                    .child(dock_tab_btn(
                        "dock-palettes",
                        "Palettes",
                        self.left_dock_tab == LeftDockTab::Palettes,
                        cx.listener(|this, _, _window, cx| {
                            this.left_dock_tab = LeftDockTab::Palettes;
                            cx.notify();
                        }),
                    ))
                    .child(dock_tab_btn(
                        "dock-workbook",
                        "Workbook",
                        self.left_dock_tab == LeftDockTab::Workbook,
                        cx.listener(|this, _, _window, cx| {
                            this.left_dock_tab = LeftDockTab::Workbook;
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .id("left-dock-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(match self.left_dock_tab {
                        LeftDockTab::Palettes => self.render_palettes_pane(cx).into_any_element(),
                        LeftDockTab::Workbook => self.render_workbook_pane(cx).into_any_element(),
                    }),
            )
    }

    fn render_palettes_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let expr_items = [
            ("a + b", "a + b"),
            ("a / b", "a / b"),
            ("a^b", "a^b"),
            ("sqrt(x)", "sqrt(x)"),
            ("abs(x)", "abs(x)"),
            ("factorial(n)", "factorial(n)"),
            ("expand(%)", "expand(%)"),
            ("factor(%)", "factor(%)"),
            ("simplify(%)", "simplify(%)"),
            ("subs(x=1,%)", "subs(x = 1, %)"),
        ];
        let calc_items = [
            "diff(f, x);",
            "diff(f, x$2);",
            "int(f, x);",
            "int(f, x = 0..1);",
            "limit(sin(x)/x, x = 0);",
            "series(exp(x), x = 0, 5);",
            "solve(x^2 - 5*x + 6 = 0, x);",
            "plot([sin(x), cos(x)], x = -Pi..Pi);",
        ];
        let symbols = [
            "Pi", "e", "I", "infinity", "alpha", "beta", "gamma", "theta", "lambda", "sigma",
            "omega",
        ];

        let mut vars: Vec<(String, String)> = Vec::new();
        if let Some(ws) = self.current_tab().worksheet() {
            for (k, v) in &ws.env.variables {
                vars.push((k.clone(), v.to_string()));
            }
            for (name, f) in &ws.env.functions {
                vars.push((
                    name.clone(),
                    format!("({}) -> {}", f.params.join(", "), f.body),
                ));
            }
        }

        let matrix_kind_label = match self.matrix_kind {
            MatrixPresetKind::Zero => "Zero (0)",
            MatrixPresetKind::Identity => "Identity (I)",
            MatrixPresetKind::Sequential => "Sequential (1..n)",
            MatrixPresetKind::Symbolic => "Symbolic (aij)",
        };

        div()
            .flex()
            .flex_col()
            .gap_1p5()
            // Expression palette
            .child(accordion_header(
                "pal-expr",
                "Expression",
                self.palettes.expression,
                cx.listener(|this, _, _window, cx| {
                    this.palettes.expression = !this.palettes.expression;
                    cx.notify();
                }),
            ))
            .when(self.palettes.expression, |el| {
                el.child(
                    div()
                        .px_1()
                        .pb_1()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap_1()
                        .children(expr_items.into_iter().enumerate().map(|(i, (lbl, tmpl))| {
                            div()
                                .id(("expr-tmpl", i))
                                .w(px(116.))
                                .px_2()
                                .py_1()
                                .rounded_sm()
                                .bg(rgb(0xffffff))
                                .border_1()
                                .border_color(rgb(0xdbe3ed))
                                .hover(|s| s.bg(rgb(0xeff6ff)).border_color(rgb(0x93c5fd)))
                                .cursor_pointer()
                                .text_xs()
                                .text_color(rgb(0x0f172a))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.insert_text(tmpl, cx);
                                    window.focus(&this.focus_handle);
                                }))
                                .child(lbl)
                        })),
                )
            })
            // Calculus palette
            .child(accordion_header(
                "pal-calc",
                "Calculus",
                self.palettes.calculus,
                cx.listener(|this, _, _window, cx| {
                    this.palettes.calculus = !this.palettes.calculus;
                    cx.notify();
                }),
            ))
            .when(self.palettes.calculus, |el| {
                el.child(div().px_1().pb_1().flex().flex_col().gap_1().children(
                    calc_items.into_iter().enumerate().map(|(i, cmd)| {
                        div()
                            .id(("calc-tmpl", i))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xdbe3ed))
                            .hover(|s| s.bg(rgb(0xeff6ff)).border_color(rgb(0x93c5fd)))
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(0x1e293b))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_input_and_focus(cmd, window, cx);
                            }))
                            .child(cmd)
                    }),
                ))
            })
            // Variables table palette (matches Variable | Value table in Maple 2025)
            .child(accordion_header(
                "pal-vars",
                "Variables",
                self.palettes.variables,
                cx.listener(|this, _, _window, cx| {
                    this.palettes.variables = !this.palettes.variables;
                    cx.notify();
                }),
            ))
            .when(self.palettes.variables, |el| {
                el.child(
                    div()
                        .mx_1()
                        .mb_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(0xd4dde7))
                        .bg(rgb(0xffffff))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .bg(rgb(0xf1f5f9))
                                .border_b_1()
                                .border_color(rgb(0xd4dde7))
                                .flex()
                                .flex_row()
                                .justify_between()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(0x475569))
                                .child("Variable")
                                .child("Value"),
                        )
                        .child(if vars.is_empty() {
                            div()
                                .px_2()
                                .py_2()
                                .text_xs()
                                .text_color(rgb(0x94a3b8))
                                .child("No variables in current sheet")
                                .into_any_element()
                        } else {
                            div()
                                .flex()
                                .flex_col()
                                .children(vars.into_iter().enumerate().map(|(idx, (k, v))| {
                                    let ins = k.clone();
                                    div()
                                        .id(("var-row", idx))
                                        .px_2()
                                        .py_1()
                                        .border_t_1()
                                        .border_color(rgb(0xf1f5f9))
                                        .hover(|s| s.bg(rgb(0xeff6ff)))
                                        .cursor_pointer()
                                        .flex()
                                        .flex_row()
                                        .justify_between()
                                        .gap_2()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.insert_text(&ins, cx);
                                            window.focus(&this.focus_handle);
                                        }))
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(0x0f172a))
                                                .child(k),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0x1d4ed8))
                                                .overflow_hidden()
                                                .child(v),
                                        )
                                }))
                                .into_any_element()
                        }),
                )
            })
            // Matrix Palette (mirrors Maple 2025 Matrix Rows/Columns/Type/Insert Matrix)
            .child(accordion_header(
                "pal-matrix",
                "Matrix",
                self.palettes.matrix,
                cx.listener(|this, _, _window, cx| {
                    this.palettes.matrix = !this.palettes.matrix;
                    cx.notify();
                }),
            ))
            .when(self.palettes.matrix, |el| {
                el.child(
                    div()
                        .mx_1()
                        .mb_1()
                        .p_2()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(0xd4dde7))
                        .bg(rgb(0xffffff))
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(matrix_stepper(
                            "Rows",
                            self.matrix_rows,
                            "mat-row-dec",
                            "mat-row-inc",
                            cx.listener(|this, _, _window, cx| {
                                this.matrix_rows = (this.matrix_rows.saturating_sub(1)).max(1);
                                cx.notify();
                            }),
                            cx.listener(|this, _, _window, cx| {
                                this.matrix_rows = (this.matrix_rows + 1).min(6);
                                cx.notify();
                            }),
                        ))
                        .child(matrix_stepper(
                            "Columns",
                            self.matrix_cols,
                            "mat-col-dec",
                            "mat-col-inc",
                            cx.listener(|this, _, _window, cx| {
                                this.matrix_cols = (this.matrix_cols.saturating_sub(1)).max(1);
                                cx.notify();
                            }),
                            cx.listener(|this, _, _window, cx| {
                                this.matrix_cols = (this.matrix_cols + 1).min(6);
                                cx.notify();
                            }),
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(div().text_xs().text_color(rgb(0x475569)).child("Type:"))
                                .child(
                                    div()
                                        .id("mat-kind-cycle")
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(rgb(0xf1f5f9))
                                        .border_1()
                                        .border_color(rgb(0xcbd5e1))
                                        .hover(|s| s.bg(rgb(0xe2e8f0)))
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(rgb(0x0f172a))
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.cycle_matrix_kind(cx);
                                        }))
                                        .child(matrix_kind_label),
                                ),
                        )
                        .child(
                            div()
                                .id("btn-insert-matrix")
                                .w_full()
                                .py_1()
                                .rounded_sm()
                                .bg(rgb(0x1d4ed8))
                                .hover(|s| s.bg(rgb(0x1e40af)))
                                .cursor_pointer()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(0xffffff))
                                .flex()
                                .justify_center()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.insert_matrix_from_palette(window, cx);
                                }))
                                .child("Insert Matrix"),
                        ),
                )
            })
            // Common Symbols & Greek
            .child(accordion_header(
                "pal-symbols",
                "Common Symbols & Greek",
                self.palettes.symbols,
                cx.listener(|this, _, _window, cx| {
                    this.palettes.symbols = !this.palettes.symbols;
                    cx.notify();
                }),
            ))
            .when(self.palettes.symbols, |el| {
                el.child(
                    div()
                        .px_1()
                        .pb_1()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap_1()
                        .children(symbols.into_iter().enumerate().map(|(i, sym)| {
                            div()
                                .id(("sym", i))
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .bg(rgb(0xffffff))
                                .border_1()
                                .border_color(rgb(0xdbe3ed))
                                .hover(|s| s.bg(rgb(0xeff6ff)))
                                .cursor_pointer()
                                .text_xs()
                                .text_color(rgb(0x1e293b))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.insert_text(sym, cx);
                                    window.focus(&this.focus_handle);
                                }))
                                .child(sym)
                        })),
                )
            })
    }

    fn render_workbook_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let starters = [
            ("Calculus Worksheet", "calculus"),
            ("Linear Algebra Worksheet", "linalg"),
            ("Series & Limits Worksheet", "series"),
            ("2D Plotting Worksheet", "plot"),
        ];

        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_1()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x64748b))
                            .child("OPEN DOCUMENTS"),
                    )
                    .children(self.tabs.iter().enumerate().map(|(idx, tab)| {
                        let active = idx == self.active_tab;
                        div()
                            .id(("wb-tab", idx))
                            .px_2()
                            .py_1p5()
                            .rounded_sm()
                            .bg(if active { rgb(0xdbeafe) } else { rgb(0xffffff) })
                            .border_1()
                            .border_color(if active { rgb(0x60a5fa) } else { rgb(0xdbe3ed) })
                            .hover(|s| s.bg(rgb(0xeff6ff)))
                            .cursor_pointer()
                            .text_xs()
                            .font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(rgb(0x0f172a))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.active_tab = idx;
                                window.focus(&this.focus_handle);
                                cx.notify();
                            }))
                            .child(tab.title.clone())
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x64748b))
                            .child("STARTER WORKSHEETS"),
                    )
                    .children(starters.into_iter().enumerate().map(|(idx, (label, key))| {
                        div()
                            .id(("wb-starter", idx))
                            .px_2()
                            .py_1p5()
                            .rounded_sm()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xdbe3ed))
                            .hover(|s| s.bg(rgb(0xeff6ff)).border_color(rgb(0x93c5fd)))
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(0x1e293b))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_starter_worksheet(Some(key), window, cx);
                            }))
                            .child(label)
                    }))
                    .child(
                        div()
                            .id("wb-new-empty")
                            .mt_1()
                            .px_2()
                            .py_1p5()
                            .rounded_sm()
                            .bg(rgb(0x1d4ed8))
                            .hover(|s| s.bg(rgb(0x1e40af)))
                            .cursor_pointer()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xffffff))
                            .flex()
                            .justify_center()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_starter_worksheet(None, window, cx);
                            }))
                            .child("+ New Empty Worksheet"),
                    ),
            )
    }

    fn render_center_workspace(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .h_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(0xeef2f6))
            // Document tab bar
            .child(
                div()
                    .w_full()
                    .h(px(32.))
                    .px_2()
                    .bg(rgb(0xe4ebf2))
                    .border_b_1()
                    .border_color(rgb(0xd4dde7))
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap_1()
                    .children(self.tabs.iter().enumerate().map(|(idx, tab)| {
                        let active = idx == self.active_tab;
                        let closable = idx > 0;
                        div()
                            .id(("doc-tab", tab.id))
                            .h(px(28.))
                            .px_2p5()
                            .rounded_t_md()
                            .bg(if active { rgb(0xffffff) } else { rgb(0xdbe3ec) })
                            .border_t_1()
                            .border_l_1()
                            .border_r_1()
                            .border_color(if active { rgb(0xcbd5e1) } else { rgb(0xd4dde7) })
                            .cursor_pointer()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.active_tab = idx;
                                window.focus(&this.focus_handle);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(if active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(rgb(0x0f172a))
                                    .child(tab.title.clone()),
                            )
                            .when(closable, |el| {
                                el.child(
                                    div()
                                        .id(("doc-close", tab.id))
                                        .px_1()
                                        .rounded_sm()
                                        .text_xs()
                                        .text_color(rgb(0x64748b))
                                        .hover(|s| s.bg(rgb(0xfee2e2)).text_color(rgb(0xb91c1c)))
                                        .on_click(cx.listener(move |this, _, _window, cx| {
                                            this.close_tab(idx, cx);
                                        }))
                                        .child("×"),
                                )
                            })
                    }))
                    .child(
                        div()
                            .id("doc-tab-add")
                            .h(px(24.))
                            .px_2()
                            .mb_0p5()
                            .rounded_sm()
                            .hover(|s| s.bg(rgb(0xcbd5e1)))
                            .cursor_pointer()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x334155))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_starter_worksheet(None, window, cx);
                            }))
                            .child("+"),
                    ),
            )
            // Document body
            .child(match &self.current_tab().content {
                DocumentContent::StartPage => self.render_start_page(cx).into_any_element(),
                DocumentContent::Worksheet(ws) => {
                    self.render_worksheet_document(ws, cx).into_any_element()
                }
            })
    }

    fn render_start_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let quick_cards = [
            (
                "Symbolic Calculus",
                "Differentiate, integrate, compute L'Hôpital limits, and expand functions.",
                "calculus",
                rgb(0x1d4ed8),
            ),
            (
                "Exact Linear Algebra",
                "Rational matrices, determinants, inverses, exact RREF, and eigenvalues.",
                "linalg",
                rgb(0x0f766e),
            ),
            (
                "Taylor Series & Equations",
                "Maclaurin/Taylor expansions and closed-form polynomial/symbolic solving.",
                "series",
                rgb(0x7c3aed),
            ),
            (
                "2D Vector Plotting",
                "Multi-curve real function plots rendered directly on the GPU canvas.",
                "plot",
                rgb(0xc81e32),
            ),
        ];

        div()
            .id("start-page-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_6()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .rounded_lg()
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0xd4dde7))
                    .p_8()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .items_center()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0x0f172a))
                                    .child("Welcome to Maple Worksheet"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x475569))
                                    .child("Lightweight native symbolic algebra, calculus, exact linear algebra, and 2D plotting"),
                            ),
                    )
                    // Primary launch buttons matching Maple 2025 Start.mw
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_center()
                            .gap_5()
                            .child(
                                div()
                                    .id("start-new-worksheet")
                                    .w(px(210.))
                                    .p_4()
                                    .rounded_lg()
                                    .bg(rgb(0xf8fafc))
                                    .border_1()
                                    .border_color(rgb(0xcbd5e1))
                                    .hover(|s| s.bg(rgb(0xeff6ff)).border_color(rgb(0x3b82f6)))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap_2()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_starter_worksheet(None, window, cx);
                                    }))
                                    .child(
                                        div()
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .bg(rgb(0x1d4ed8))
                                            .text_base()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0xffffff))
                                            .child("[>"),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0x0f172a))
                                            .child("New Worksheet"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x64748b))
                                            .child("Interactive [> prompt mode"),
                                    ),
                            )
                            .child(
                                div()
                                    .id("start-sample-session")
                                    .w(px(210.))
                                    .p_4()
                                    .rounded_lg()
                                    .bg(rgb(0xf8fafc))
                                    .border_1()
                                    .border_color(rgb(0xcbd5e1))
                                    .hover(|s| s.bg(rgb(0xecfdf5)).border_color(rgb(0x10b981)))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap_2()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_starter_worksheet(Some("calculus"), window, cx);
                                    }))
                                    .child(
                                        div()
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .bg(rgb(0x059669))
                                            .text_base()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0xffffff))
                                            .child("+"),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0x0f172a))
                                            .child("Sample Session"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x64748b))
                                            .child("Pre-loaded calculus examples"),
                                    ),
                            ),
                    )
                    // Topic starter cards
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0x64748b))
                                    .child("QUICK START WORKSHEETS"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
                                    .gap_3()
                                    .children(quick_cards.into_iter().enumerate().map(
                                        |(idx, (title, desc, key, accent))| {
                                            div()
                                                .id(("start-card", idx))
                                                .w(px(336.))
                                                .p_3()
                                                .rounded_md()
                                                .bg(rgb(0xf8fafc))
                                                .border_1()
                                                .border_color(rgb(0xe2e8f0))
                                                .hover(|s| {
                                                    s.bg(rgb(0xf1f5f9))
                                                        .border_color(rgb(0x94a3b8))
                                                })
                                                .cursor_pointer()
                                                .flex()
                                                .flex_col()
                                                .gap_1()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    this.open_starter_worksheet(
                                                        Some(key),
                                                        window,
                                                        cx,
                                                    );
                                                }))
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(accent)
                                                        .child(title),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(0x475569))
                                                        .child(desc),
                                                )
                                        },
                                    )),
                            ),
                    ),
            )
    }

    fn render_worksheet_document(
        &self,
        ws: &Worksheet,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tab = self.current_tab();
        let before = tab.input[..tab.cursor].to_string();
        let after = tab.input[tab.cursor..].to_string();
        let is_empty = tab.input.is_empty();
        let selected = tab.selected_cell;

        div()
            .id("worksheet-doc-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&tab.scroll_handle)
            .p_5()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(860.))
                    .min_h(px(640.))
                    .rounded_md()
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0xd4dde7))
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .when(ws.cells.is_empty(), |el| {
                        el.child(
                            div()
                                .pb_2()
                                .text_xs()
                                .text_color(rgb(0x64748b))
                                .child("Type a Maple command below (e.g., f := x^3 * sin(x); or rref(Matrix([[1,2,3],[4,5,6]]));) and press Enter."),
                        )
                    })
                    .children(ws.cells.iter().enumerate().map(|(idx, cell)| {
                        let is_selected = selected == Some(idx);
                        let cell_input = cell.input.clone();
                        div()
                            .id(("cell", idx))
                            .w_full()
                            .pl_2()
                            .pr_2()
                            .py_1p5()
                            .rounded_sm()
                            .bg(if is_selected {
                                rgb(0xf8fbff)
                            } else {
                                rgb(0xffffff)
                            })
                            .border_l_2()
                            .border_color(if is_selected {
                                rgb(0x2563eb)
                            } else {
                                rgb(0xcbd5e1)
                            })
                            .hover(|s| s.bg(rgb(0xf8fafc)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let t = this.current_tab_mut();
                                t.selected_cell = Some(idx);
                                if t.input.is_empty() {
                                    t.input = cell_input.clone();
                                    t.cursor = t.input.len();
                                }
                                window.focus(&this.focus_handle);
                                cx.notify();
                            }))
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(rgb(0xb91c1c))
                                                    .child("[>"),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(0x0f172a))
                                                    .child(cell.input.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(0x94a3b8))
                                                    .child(format!("({})", cell.index)),
                                            )
                                            .child(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_sm()
                                                    .bg(rgb(0xf1f5f9))
                                                    .text_xs()
                                                    .text_color(rgb(0x64748b))
                                                    .child(format!("{} μs", cell.elapsed_us)),
                                            ),
                                    ),
                            )
                            .child(render_cell_output(&cell.output))
                    }))
                    // Active inline Maple `[>` execution group at bottom of document
                    .child(
                        div()
                            .mt_2()
                            .pl_2()
                            .py_2()
                            .border_l_2()
                            .border_color(rgb(0xb91c1c))
                            .bg(rgb(0xfcfcfd))
                            .rounded_r_sm()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0xb91c1c))
                                    .child("[>"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .text_sm()
                                    .text_color(rgb(0x0f172a))
                                    .child(before)
                                    .child(div().w(px(2.)).h(px(17.)).bg(rgb(0xb91c1c)))
                                    .child(if is_empty {
                                        div()
                                            .pl_2()
                                            .text_color(rgb(0x94a3b8))
                                            .child("Enter Maple command...")
                                            .into_any_element()
                                    } else {
                                        div().child(after).into_any_element()
                                    }),
                            )
                            .child(
                                div()
                                    .id("inline-eval-btn")
                                    .px_2p5()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgb(0x1d4ed8))
                                    .hover(|s| s.bg(rgb(0x1e40af)))
                                    .cursor_pointer()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.evaluate_input(&Evaluate, window, cx);
                                    }))
                                    .child("Evaluate"),
                            ),
                    ),
            )
    }

    fn render_right_dock(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let context_actions = self.current_tab().worksheet().and_then(|ws| {
            let idx = self
                .current_tab()
                .selected_cell
                .filter(|&i| i < ws.cells.len())
                .or_else(|| ws.cells.len().checked_sub(1))?;
            Some((ws.cells[idx].index, ws.cells[idx].context_actions()))
        });

        let shortcuts = [
            ("Evaluate", "Enter"),
            ("Execute Worksheet", "Ctrl+Shift+Enter"),
            ("Toggle Math/Text", "F5"),
            ("Restart Kernel", "Ctrl+R"),
            ("Clear Sheet", "Ctrl+L"),
            ("New Worksheet", "Ctrl+N"),
            ("Assignments", "a := b"),
            ("Functions", "f := x -> x^2"),
            ("Equations", "x = y"),
            ("History", "Up / Down"),
        ];

        div()
            .w(px(252.))
            .h_full()
            .bg(rgb(0xf8fafc))
            .border_l_1()
            .border_color(rgb(0xd4dde7))
            .p_2p5()
            .flex()
            .flex_col()
            .gap_3()
            // Context Panel (matches Maple 2025 right dock)
            .child(
                div()
                    .rounded_md()
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0xd4dde7))
                    .p_2p5()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x334155))
                            .child("Context Panel"),
                    )
                    .child(match context_actions {
                        Some((cell_idx, actions)) if !actions.is_empty() => div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x64748b))
                                    .child(format!("Actions for cell ({cell_idx}):")),
                            )
                            .children(actions.into_iter().enumerate().map(|(i, (label, cmd))| {
                                div()
                                    .id(("ctx-act", i))
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgb(0xf8fafc))
                                    .border_1()
                                    .border_color(rgb(0xe2e8f0))
                                    .hover(|s| s.bg(rgb(0xeff6ff)).border_color(rgb(0x93c5fd)))
                                    .cursor_pointer()
                                    .text_xs()
                                    .text_color(rgb(0x1d4ed8))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.run_context_action(&cmd, window, cx);
                                    }))
                                    .child(label)
                            }))
                            .into_any_element(),
                        _ => div()
                            .text_xs()
                            .text_color(rgb(0x94a3b8))
                            .child("Select or evaluate an expression or matrix to inspect context actions.")
                            .into_any_element(),
                    }),
            )
            // Math Editor Shortcuts (matches right panel in Maple 2025 screenshot)
            .child(
                div()
                    .rounded_md()
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0xd4dde7))
                    .p_2p5()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .pb_1()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x334155))
                            .child("Math Editor Shortcuts"),
                    )
                    .children(shortcuts.into_iter().map(|(name, key)| {
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .gap_2()
                            .text_xs()
                            .child(div().text_color(rgb(0x475569)).child(name))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(0x0f172a))
                                    .child(key),
                            )
                    })),
            )
    }

    fn render_status_bar(&self) -> impl IntoElement {
        let tab = self.current_tab();
        let symbols = tab
            .worksheet()
            .map(|ws| ws.env.variables.len() + ws.env.functions.len())
            .unwrap_or(0);
        let time_str = tab
            .last_elapsed_us
            .map(|us| format!("{us} μs"))
            .unwrap_or_else(|| "0 μs".to_string());
        let mode_str = match self.entry_mode {
            EntryMode::Math => "Math Mode",
            EntryMode::Text => "Text Mode",
        };

        div()
            .w_full()
            .h(px(24.))
            .px_3()
            .bg(rgb(0xeaf0f6))
            .border_t_1()
            .border_color(rgb(0xd4dde7))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .text_xs()
            .text_color(rgb(0x475569))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().size_2().rounded_full().bg(rgb(0x10b981)))
                    .child(self.status_message.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .child("Editable")
                    .child(tab.title.clone())
                    .child(format!("Symbols: {symbols}"))
                    .child(format!("Time: {time_str}"))
                    .child("Zoom: 100%")
                    .child(mode_str),
            )
    }
}

impl Render for MapleWorkspaceView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::evaluate_input))
            .on_action(cx.listener(Self::execute_worksheet))
            .on_action(cx.listener(Self::toggle_math_text_mode))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::move_home))
            .on_action(cx.listener(Self::move_end))
            .on_action(cx.listener(Self::history_prev))
            .on_action(cx.listener(Self::history_next))
            .on_action(cx.listener(Self::clear_worksheet))
            .on_action(cx.listener(Self::restart_kernel))
            .on_action(cx.listener(Self::new_worksheet_action))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy_last_output))
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xeef2f6))
            .child(self.render_menu_strip(cx))
            .child(self.render_ribbon(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_row()
                    .child(self.render_left_dock(cx))
                    .child(self.render_center_workspace(cx))
                    .when(self.right_dock_open, |el| {
                        el.child(self.render_right_dock(cx))
                    }),
            )
            .child(self.render_status_bar())
    }
}

fn ribbon_btn(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    accent: Rgba,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2p5()
        .py_1()
        .rounded_md()
        .bg(rgb(0xffffff))
        .border_1()
        .border_color(rgb(0xd4dde7))
        .hover(|s| s.bg(rgb(0xeff6ff)).border_color(rgb(0x93c5fd)))
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .on_click(on_click)
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(accent)
                .child(icon),
        )
        .child(div().text_xs().text_color(rgb(0x1e293b)).child(label))
}

fn dock_tab_btn(
    id: &'static str,
    label: &'static str,
    active: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .h_full()
        .bg(if active { rgb(0xf8fafc) } else { rgb(0xe2e8f0) })
        .border_b_2()
        .border_color(if active { rgb(0x1d4ed8) } else { rgb(0xd4dde7) })
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(rgb(0x0f172a))
        .on_click(on_click)
        .child(label)
}

fn accordion_header(
    id: &'static str,
    title: &'static str,
    expanded: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let glyph = if expanded { "▾" } else { "▸" };
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(0xeaf0f6))
        .hover(|s| s.bg(rgb(0xdbe5f0)))
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .on_click(on_click)
        .child(div().text_xs().text_color(rgb(0x475569)).child(glyph))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x1e293b))
                .child(title),
        )
}

fn matrix_stepper(
    label: &'static str,
    value: usize,
    dec_id: &'static str,
    inc_id: &'static str,
    on_dec: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_inc: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_xs().text_color(rgb(0x475569)).child(label))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .id(dec_id)
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(rgb(0xf1f5f9))
                        .border_1()
                        .border_color(rgb(0xcbd5e1))
                        .hover(|s| s.bg(rgb(0xe2e8f0)))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(rgb(0x0f172a))
                        .on_click(on_dec)
                        .child("-"),
                )
                .child(
                    div()
                        .w(px(22.))
                        .flex()
                        .justify_center()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0x0f172a))
                        .child(value.to_string()),
                )
                .child(
                    div()
                        .id(inc_id)
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(rgb(0xf1f5f9))
                        .border_1()
                        .border_color(rgb(0xcbd5e1))
                        .hover(|s| s.bg(rgb(0xe2e8f0)))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(rgb(0x0f172a))
                        .on_click(on_inc)
                        .child("+"),
                ),
        )
}

fn render_cell_output(output: &CellOutput) -> gpui::AnyElement {
    match output {
        CellOutput::Text(text) => div()
            .pl_6()
            .text_base()
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(0x1d4ed8))
            .child(text.clone())
            .into_any_element(),
        CellOutput::MatrixGrid(rows) => render_matrix_grid(rows).into_any_element(),
        CellOutput::Plot(plot) => render_plot(plot).into_any_element(),
        CellOutput::Suppressed => div()
            .pl_6()
            .text_xs()
            .text_color(rgb(0x94a3b8))
            .child("(output suppressed by ':')")
            .into_any_element(),
        CellOutput::Error(err) => div()
            .pl_6()
            .p_2()
            .rounded_sm()
            .bg(rgb(0xfef2f2))
            .border_1()
            .border_color(rgb(0xfecaca))
            .text_sm()
            .text_color(rgb(0xb91c1c))
            .child(format!("Error: {err}"))
            .into_any_element(),
    }
}

fn render_matrix_grid(rows: &[Vec<String>]) -> impl IntoElement {
    div().pl_6().flex().flex_row().items_start().child(
        div()
            .px_3()
            .py_1p5()
            .border_l_2()
            .border_r_2()
            .border_color(rgb(0x1d4ed8))
            .rounded_sm()
            .flex()
            .flex_col()
            .gap_1()
            .children(rows.iter().map(|row| {
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .children(row.iter().map(|cell| {
                        div()
                            .min_w(px(44.))
                            .text_sm()
                            .text_color(rgb(0x1d4ed8))
                            .font_weight(FontWeight::MEDIUM)
                            .child(cell.clone())
                    }))
            })),
    )
}

fn render_plot(plot: &PlotData) -> impl IntoElement {
    let palette = [rgb(0x2563eb), rgb(0xdc2626), rgb(0x059669), rgb(0xd97706)];
    let plot_data = plot.clone();

    div()
        .pl_6()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0x334155))
                        .child(plot.title.clone()),
                )
                .child(div().flex().flex_row().gap_3().children(
                    plot.series.iter().enumerate().map(|(i, s)| {
                        let color = palette[i % palette.len()];
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(div().w(px(10.)).h(px(3.)).bg(color))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x475569))
                                    .child(s.name.clone()),
                            )
                    }),
                )),
        )
        .child(
            div()
                .w_full()
                .h(px(230.))
                .rounded_md()
                .bg(rgb(0xf8fafc))
                .border_1()
                .border_color(rgb(0xcbd5e1))
                .child(
                    canvas(
                        move |_bounds, _window, _cx| plot_data.clone(),
                        move |bounds, data, window, _cx| {
                            paint_plot_canvas(bounds, &data, &palette, window);
                        },
                    )
                    .size_full(),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .text_xs()
                .text_color(rgb(0x64748b))
                .child(format!(
                    "{} in [{:.3}, {:.3}]",
                    plot.x_label, plot.x_min, plot.x_max
                ))
                .child(format!("y in [{:.3}, {:.3}]", plot.y_min, plot.y_max)),
        )
}

fn paint_plot_canvas(
    bounds: Bounds<Pixels>,
    data: &PlotData,
    palette: &[Rgba],
    window: &mut Window,
) {
    let pad = 16.0_f32;
    let left = f32::from(bounds.left()) + pad;
    let right = f32::from(bounds.right()) - pad;
    let top = f32::from(bounds.top()) + pad;
    let bottom = f32::from(bounds.bottom()) - pad;
    let width = (right - left).max(1.0);
    let height = (bottom - top).max(1.0);

    let x_span = (data.x_max - data.x_min).max(1e-9);
    let y_span = (data.y_max - data.y_min).max(1e-9);

    let map_x = |x: f64| -> Pixels { px(left + (((x - data.x_min) / x_span) as f32) * width) };
    let map_y = |y: f64| -> Pixels { px(bottom - (((y - data.y_min) / y_span) as f32) * height) };

    for step in 1..4 {
        let frac = step as f32 / 4.0;
        let gx = px(left + frac * width);
        let gy = px(top + frac * height);
        window.paint_quad(fill(
            Bounds::new(point(gx, px(top)), size(px(1.0), px(height))),
            rgb(0xe2e8f0),
        ));
        window.paint_quad(fill(
            Bounds::new(point(px(left), gy), size(px(width), px(1.0))),
            rgb(0xe2e8f0),
        ));
    }

    if data.x_min <= 0.0 && 0.0 <= data.x_max {
        let ax = map_x(0.0);
        window.paint_quad(fill(
            Bounds::new(point(ax, px(top)), size(px(1.5), px(height))),
            rgb(0x94a3b8),
        ));
    }
    if data.y_min <= 0.0 && 0.0 <= data.y_max {
        let ay = map_y(0.0);
        window.paint_quad(fill(
            Bounds::new(point(px(left), ay), size(px(width), px(1.5))),
            rgb(0x94a3b8),
        ));
    }

    for (idx, series) in data.series.iter().enumerate() {
        let color: Hsla = palette[idx % palette.len()].into();
        let mut builder = PathBuilder::stroke(px(2.0));
        let mut prev_y: Option<f64> = None;
        let mut started = false;
        for &(x, y) in &series.points {
            let sx = map_x(x);
            let sy = map_y(y);
            let jump = prev_y.is_some_and(|py| (y - py).abs() > y_span * 0.6);
            if !started || jump {
                builder.move_to(point(sx, sy));
                started = true;
            } else {
                builder.line_to(point(sx, sy));
            }
            prev_y = Some(y);
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}

fn prev_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 {
        i -= 1;
        if s.is_char_boundary(i) {
            return i;
        }
    }
    0
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i < s.len() {
        i += 1;
        if s.is_char_boundary(i) {
            return i;
        }
    }
    s.len()
}

/// Launch the interactive GPUI Maple worksheet window.
pub fn run_gui() {
    gpui::Application::new().run(|cx: &mut App| {
        register_keybindings(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx| {
            cx.quit();
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1280.0), px(840.0)), cx);
        let window = cx
            .open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("Maple Worksheet — [Local Kernel]".into()),
                        ..Default::default()
                    }),
                    focus: true,
                    ..Default::default()
                },
                |_window, cx| cx.new(MapleWorkspaceView::new),
            )
            .expect("failed to open GPUI Maple window");

        window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx));
                cx.activate(true);
            })
            .ok();
    });
}
