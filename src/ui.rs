use crate::ast::{Expr, to_latex};
use crate::plot::{Plot3dData, PlotData};
use crate::tutor::{de_presets, help_articles, simulate_de_scene, study_guide_chapters};
use crate::worksheet::{CellOutput, MatrixPresetKind, MatrixTemplate, Worksheet, WorksheetCell};
use gpui::{
    AnyView, App, Bounds, ClipboardItem, Context, CursorStyle, FocusHandle, Focusable, FontWeight,
    Hsla, KeyBinding, KeyDownEvent, MouseButton, PathBuilder, Pixels, Rgba, ScrollHandle, Window,
    WindowControlArea, WindowDecorations, actions, canvas, div, fill, point, prelude::*, px, rgb,
    size,
};
use std::io::Write;
use std::process::{Command, Stdio};

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
        CloseTabAction,
        CopyAction,
        CutAction,
        PasteAction,
        UndoAction,
        RedoAction,
        ZoomIn,
        ZoomOut,
        ZoomReset,
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
        KeyBinding::new("ctrl-w", CloseTabAction, None),
        KeyBinding::new("ctrl-c", CopyAction, None),
        KeyBinding::new("ctrl-x", CutAction, None),
        KeyBinding::new("ctrl-v", PasteAction, None),
        KeyBinding::new("ctrl-z", UndoAction, None),
        KeyBinding::new("ctrl-y", RedoAction, None),
        KeyBinding::new("ctrl-=", ZoomIn, None),
        KeyBinding::new("ctrl-+", ZoomIn, None),
        KeyBinding::new("ctrl--", ZoomOut, None),
        KeyBinding::new("ctrl-0", ZoomReset, None),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuTab {
    File,
    Home,
    Insert,
    Edit,
    View,
    Evaluation,
    Tools,
    Education,
    Workbook,
    Help,
}

impl MenuTab {
    const ALL: [MenuTab; 10] = [
        MenuTab::File,
        MenuTab::Home,
        MenuTab::Insert,
        MenuTab::Edit,
        MenuTab::View,
        MenuTab::Evaluation,
        MenuTab::Tools,
        MenuTab::Education,
        MenuTab::Workbook,
        MenuTab::Help,
    ];

    fn label(self) -> &'static str {
        match self {
            MenuTab::File => "File",
            MenuTab::Home => "Home",
            MenuTab::Insert => "Insert",
            MenuTab::Edit => "Edit",
            MenuTab::View => "View",
            MenuTab::Evaluation => "Evaluation",
            MenuTab::Tools => "Tools",
            MenuTab::Education => "Education",
            MenuTab::Workbook => "Workbook",
            MenuTab::Help => "Help",
        }
    }
}

enum DocumentContent {
    StartPage,
    Worksheet(Box<Worksheet>),
    DePlotTutor {
        preset_idx: usize,
        x0: f64,
        y0: f64,
        show_arrows: bool,
    },
    StudyGuide {
        chapter_idx: usize,
    },
    HelpBrowser {
        article_idx: usize,
    },
}

#[derive(Clone)]
struct PromptSnapshot {
    input: String,
    cursor: usize,
    cell_inputs: Option<Vec<String>>,
}

struct WorksheetTab {
    id: usize,
    title: String,
    content: DocumentContent,
    selected_cell: Option<usize>,
    editing_cell: Option<usize>,
    plot2d_zoom: f64,
    plot2d_pan_x: f64,
    plot3d_yaw_deg: f64,
    plot3d_pitch_deg: f64,
    input: String,
    cursor: usize,
    history: Vec<String>,
    history_pos: Option<usize>,
    undo_stack: Vec<PromptSnapshot>,
    redo_stack: Vec<PromptSnapshot>,
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
            editing_cell: None,
            plot2d_zoom: 1.0,
            plot2d_pan_x: 0.0,
            plot3d_yaw_deg: 42.0,
            plot3d_pitch_deg: 26.0,
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
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
            editing_cell: None,
            plot2d_zoom: 1.0,
            plot2d_pan_x: 0.0,
            plot3d_yaw_deg: 42.0,
            plot3d_pitch_deg: 26.0,
            input: String::new(),
            cursor: 0,
            history,
            history_pos: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_elapsed_us,
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn deplot_tutor(id: usize) -> Self {
        let p = &de_presets()[0];
        Self {
            id,
            title: "DEplot[interactive]".to_string(),
            content: DocumentContent::DePlotTutor {
                preset_idx: 0,
                x0: p.default_ic.0,
                y0: p.default_ic.1,
                show_arrows: true,
            },
            selected_cell: None,
            editing_cell: None,
            plot2d_zoom: 1.0,
            plot2d_pan_x: 0.0,
            plot3d_yaw_deg: 42.0,
            plot3d_pitch_deg: 26.0,
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_elapsed_us: None,
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn study_guide(id: usize, chapter_idx: usize) -> Self {
        let title = if chapter_idx == 0 {
            "StudyGuides,Precalculus,Chapter1".to_string()
        } else {
            study_guide_chapters()
                .get(chapter_idx)
                .map(|c| c.guide.to_string())
                .unwrap_or_else(|| "Study Guide".to_string())
        };
        Self {
            id,
            title,
            content: DocumentContent::StudyGuide { chapter_idx },
            selected_cell: None,
            editing_cell: None,
            plot2d_zoom: 1.0,
            plot2d_pan_x: 0.0,
            plot3d_yaw_deg: 42.0,
            plot3d_pitch_deg: 26.0,
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_elapsed_us: None,
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn help_browser(id: usize, article_idx: usize) -> Self {
        Self {
            id,
            title: "Maple 2025 Help".to_string(),
            content: DocumentContent::HelpBrowser { article_idx },
            selected_cell: None,
            editing_cell: None,
            plot2d_zoom: 1.0,
            plot2d_pan_x: 0.0,
            plot3d_yaw_deg: 42.0,
            plot3d_pitch_deg: 26.0,
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_elapsed_us: None,
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn worksheet(&self) -> Option<&Worksheet> {
        match &self.content {
            DocumentContent::Worksheet(ws) => Some(ws),
            _ => None,
        }
    }

    fn worksheet_mut(&mut self) -> Option<&mut Worksheet> {
        match &mut self.content {
            DocumentContent::Worksheet(ws) => Some(ws),
            _ => None,
        }
    }

    fn snapshot(&self, include_cells: bool) -> PromptSnapshot {
        PromptSnapshot {
            input: self.input.clone(),
            cursor: self.cursor,
            cell_inputs: if include_cells {
                self.worksheet()
                    .map(|ws| ws.cells.iter().map(|c| c.input.clone()).collect())
            } else {
                None
            },
        }
    }

    fn push_undo(&mut self, include_cells: bool) {
        let snap = self.snapshot(include_cells);
        if self.undo_stack.len() >= 100 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snap);
        self.redo_stack.clear();
    }

    fn apply_snapshot(&mut self, snap: PromptSnapshot) {
        self.input = snap.input;
        self.cursor = snap.cursor.min(self.input.len());
        self.history_pos = None;
        if let Some(inputs) = snap.cell_inputs
            && let Some(ws) = self.worksheet_mut()
        {
            ws.restart();
            ws.clear_cells();
            for inp in inputs {
                ws.evaluate(&inp);
            }
            let len = ws.cells.len();
            let last_us = ws.cells.last().map(|c| c.elapsed_us);
            self.selected_cell = len.checked_sub(1);
            self.last_elapsed_us = last_us;
        }
    }
}

struct PaletteExpansion {
    favorites: bool,
    expression: bool,
    calculus: bool,
    variables: bool,
    matrix: bool,
    symbols: bool,
}

impl Default for PaletteExpansion {
    fn default() -> Self {
        Self {
            favorites: true,
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
    active_menu: MenuTab,
    left_dock_open: bool,
    left_dock_tab: LeftDockTab,
    palettes: PaletteExpansion,
    selected_var: Option<String>,
    matrix_rows: usize,
    matrix_cols: usize,
    matrix_kind: MatrixPresetKind,
    right_dock_open: bool,
    entry_mode: EntryMode,
    zoom_percent: u32,
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
            active_menu: MenuTab::Home,
            left_dock_open: true,
            left_dock_tab: LeftDockTab::Palettes,
            palettes: PaletteExpansion::default(),
            selected_var: None,
            matrix_rows: 2,
            matrix_cols: 2,
            matrix_kind: MatrixPresetKind::Identity,
            right_dock_open: true,
            entry_mode: EntryMode::Math,
            zoom_percent: 100,
            focus_handle: cx.focus_handle(),
            status_message: "Ready".to_string(),
        }
    }

    fn current_tab(&self) -> &WorksheetTab {
        let idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        &self.tabs[idx]
    }

    fn current_tab_mut(&mut self) -> &mut WorksheetTab {
        let idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        self.active_tab = idx;
        &mut self.tabs[idx]
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
        self.status_message = format!("Active: {}", self.current_tab().title);
        cx.notify();
    }

    fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        self.ensure_worksheet_tab();
        let clean = text.replace(['\r', '\n'], " ");
        let tab = self.current_tab_mut();
        tab.push_undo(false);
        tab.input.insert_str(tab.cursor, &clean);
        tab.cursor += clean.len();
        tab.history_pos = None;
        cx.notify();
    }

    fn set_input_and_focus(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_worksheet_tab();
        let tab = self.current_tab_mut();
        tab.push_undo(false);
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
        if stmt.contains("DEtools[DEplot]") || stmt.starts_with("DEplot") {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            self.tabs.push(WorksheetTab::deplot_tutor(id));
            self.active_tab = self.tabs.len() - 1;
            self.status_message =
                "Opened interactive DEtools[DEplot] phase-plane tutor".to_string();
            cx.notify();
            return;
        }
        self.ensure_worksheet_tab();
        let tab = self.current_tab_mut();
        tab.push_undo(true);
        let editing = tab.editing_cell.take();
        let Some(ws) = tab.worksheet_mut() else {
            return;
        };
        if let Some(edit_idx) = editing
            && ws.update_cell(edit_idx, stmt)
        {
            let last_elapsed_us = ws.cells.get(edit_idx).map(|c| c.elapsed_us);
            tab.last_elapsed_us = last_elapsed_us;
            tab.selected_cell = Some(edit_idx);
            if clear_prompt {
                tab.input.clear();
                tab.cursor = 0;
            }
            self.status_message =
                format!("Updated cell {} and re-evaluated worksheet", edit_idx + 1);
            cx.notify();
            return;
        }

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
            EntryMode::Math => "Math Mode".to_string(),
            EntryMode::Text => "Text Mode".to_string(),
        };
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        if tab.cursor == 0 {
            return;
        }
        tab.push_undo(false);
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
        tab.push_undo(false);
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
        let tab = self.current_tab_mut();
        tab.push_undo(true);
        if let Some(ws) = tab.worksheet_mut() {
            ws.clear_cells();
            self.current_tab_mut().selected_cell = None;
            self.status_message = "Cleared worksheet cells (variables retained)".to_string();
            cx.notify();
        }
    }

    fn restart_kernel(&mut self, _: &RestartKernel, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ws) = self.current_tab_mut().worksheet_mut() {
            ws.restart();
            let tab = self.current_tab_mut();
            tab.last_elapsed_us = None;
            self.selected_var = None;
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

    fn close_tab_action(
        &mut self,
        _: &CloseTabAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_tab(self.active_tab, cx);
    }

    fn copy_action(&mut self, _: &CopyAction, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.current_tab().input.is_empty() {
            let text = self.current_tab().input.clone();
            write_clipboard(&text, cx);
            self.status_message = "Copied prompt input to clipboard".to_string();
            cx.notify();
            return;
        }
        self.copy_selected_or_last_output(cx);
    }

    fn cut_action(&mut self, _: &CutAction, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        if tab.input.is_empty() {
            self.status_message = "Prompt is empty — nothing to cut".to_string();
            cx.notify();
            return;
        }
        let text = tab.input.clone();
        tab.push_undo(false);
        tab.input.clear();
        tab.cursor = 0;
        write_clipboard(&text, cx);
        self.status_message = "Cut prompt input to clipboard".to_string();
        cx.notify();
    }

    fn paste_action(&mut self, _: &PasteAction, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = read_clipboard(cx) {
            self.insert_text(&text, cx);
            self.status_message = "Pasted from clipboard".to_string();
            cx.notify();
        }
    }

    fn copy_selected_or_last_output(&mut self, cx: &mut Context<Self>) {
        let Some(ws) = self.current_tab().worksheet() else {
            return;
        };
        let target = self
            .current_tab()
            .selected_cell
            .and_then(|idx| ws.cells.get(idx))
            .or_else(|| ws.cells.last());
        let Some(cell) = target else {
            self.status_message = "No output to copy".to_string();
            cx.notify();
            return;
        };
        if let Some(text) = cell_output_clipboard_text(cell) {
            write_clipboard(&text, cx);
            self.status_message = format!("Copied output ({}) to clipboard", cell.index);
            cx.notify();
        }
    }

    fn undo_action(&mut self, _: &UndoAction, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        let Some(prev) = tab.undo_stack.pop() else {
            self.status_message = "Nothing to undo".to_string();
            cx.notify();
            return;
        };
        let redo_snap = tab.snapshot(prev.cell_inputs.is_some());
        tab.redo_stack.push(redo_snap);
        tab.apply_snapshot(prev);
        self.status_message = "Undid edit".to_string();
        cx.notify();
    }

    fn redo_action(&mut self, _: &RedoAction, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        let Some(next) = tab.redo_stack.pop() else {
            self.status_message = "Nothing to redo".to_string();
            cx.notify();
            return;
        };
        let undo_snap = tab.snapshot(next.cell_inputs.is_some());
        tab.undo_stack.push(undo_snap);
        tab.apply_snapshot(next);
        self.status_message = "Redid edit".to_string();
        cx.notify();
    }

    fn zoom_in(&mut self, _: &ZoomIn, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_percent = (self.zoom_percent + 10).min(160);
        self.status_message = format!("Zoom: {}%", self.zoom_percent);
        cx.notify();
    }

    fn zoom_out(&mut self, _: &ZoomOut, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_percent = self.zoom_percent.saturating_sub(10).max(80);
        self.status_message = format!("Zoom: {}%", self.zoom_percent);
        cx.notify();
    }

    fn zoom_reset(&mut self, _: &ZoomReset, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_percent = 100;
        self.status_message = "Zoom: 100%".to_string();
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

    fn open_deplot_tutor_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pos) = self
            .tabs
            .iter()
            .position(|t| matches!(t.content, DocumentContent::DePlotTutor { .. }))
        {
            self.active_tab = pos;
        } else {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            self.tabs.push(WorksheetTab::deplot_tutor(id));
            self.active_tab = self.tabs.len() - 1;
        }
        self.status_message = "DEtools[DEplot][interactive] tutor active".to_string();
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn open_study_guide_tab(
        &mut self,
        chapter_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pos) = self.tabs.iter().position(
            |t| matches!(t.content, DocumentContent::StudyGuide { chapter_idx: c } if c == chapter_idx),
        ) {
            self.active_tab = pos;
        } else {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            self.tabs.push(WorksheetTab::study_guide(id, chapter_idx));
            self.active_tab = self.tabs.len() - 1;
        }
        self.status_message = format!("Opened {}", self.current_tab().title);
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn open_help_browser_tab(
        &mut self,
        article_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pos) = self
            .tabs
            .iter()
            .position(|t| matches!(t.content, DocumentContent::HelpBrowser { .. }))
        {
            self.active_tab = pos;
            if let DocumentContent::HelpBrowser {
                article_idx: ref mut cur,
            } = self.tabs[pos].content
            {
                *cur = article_idx;
            }
        } else {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            self.tabs.push(WorksheetTab::help_browser(id, article_idx));
            self.active_tab = self.tabs.len() - 1;
        }
        self.status_message = "Opened Maple 2025 Help Reference".to_string();
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn open_saved_worksheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = std::path::PathBuf::from("/home/kk/Projects/maple_syrup/worksheets");
        let _ = std::fs::create_dir_all(&dir);
        let candidate = std::fs::read_dir(&dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                matches!(
                    p.extension().and_then(|s| s.to_str()),
                    Some("mpl") | Some("mw")
                )
            })
            .max();
        let path = match candidate {
            Some(p) => p,
            None => {
                let sample = dir.join("Sample_Inequalities.mpl");
                let _ = std::fs::write(
                    &sample,
                    "# Precalculus Study Guide Ch.1\nsolve(2*x - 3 < 5, x);\nsolve(abs(2*x - 3) < 5, x);\nplot3d(sin(x)*cos(y), x = -Pi..Pi, y = -Pi..Pi);\n",
                );
                sample
            }
        };
        let mut ws = Worksheet::new();
        match ws.load_file(&path) {
            Ok(cells) => {
                let id = self.next_tab_id;
                self.next_tab_id += 1;
                let title = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Opened.mpl")
                    .to_string();
                let mut tab = WorksheetTab::new_worksheet(id, title, None);
                tab.history = cells.iter().map(|c| c.input.clone()).collect();
                tab.selected_cell = cells.len().checked_sub(1);
                tab.last_elapsed_us = cells.last().map(|c| c.elapsed_us);
                tab.content = DocumentContent::Worksheet(Box::new(ws));
                self.tabs.push(tab);
                self.active_tab = self.tabs.len() - 1;
                self.status_message = format!("Loaded {}", path.display());
            }
            Err(err) => {
                self.status_message = format!("Open failed: {err}");
            }
        }
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn duplicate_active_worksheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = self.current_tab().worksheet() else {
            self.open_starter_worksheet(None, window, cx);
            return;
        };
        let inputs: Vec<String> = ws.cells.iter().map(|c| c.input.clone()).collect();
        let idx = self.open_new_worksheet_tab(None);
        let mut last_us = None;
        let mut len = 0;
        if let Some(new_ws) = self.tabs[idx].worksheet_mut() {
            for inp in inputs {
                new_ws.evaluate(&inp);
            }
            len = new_ws.cells.len();
            last_us = new_ws.cells.last().map(|c| c.elapsed_us);
        }
        self.tabs[idx].selected_cell = len.checked_sub(1);
        self.tabs[idx].last_elapsed_us = last_us;
        self.status_message = format!("Duplicated into {}", self.current_tab().title);
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn save_active_worksheet(&mut self, cx: &mut Context<Self>) {
        let Some(ws) = self.current_tab().worksheet() else {
            self.status_message = "Open a worksheet tab before saving".to_string();
            cx.notify();
            return;
        };
        let dir = std::path::PathBuf::from("/home/kk/Projects/maple_syrup/worksheets");
        if let Err(err) = std::fs::create_dir_all(&dir) {
            self.status_message = format!("Save failed: {err}");
            cx.notify();
            return;
        }
        let stem = self
            .current_tab()
            .title
            .trim_end_matches(".mw")
            .replace(' ', "_");
        let path = dir.join(format!("{stem}.mpl"));
        let mut body = String::from("# Saved Maple Worksheet script\n");
        for cell in &ws.cells {
            body.push_str(&cell.input);
            if !cell.input.ends_with(';') && !cell.input.ends_with(':') {
                body.push(';');
            }
            body.push('\n');
        }
        match std::fs::write(&path, body) {
            Ok(()) => {
                self.status_message = format!("Saved {}", path.display());
            }
            Err(err) => {
                self.status_message = format!("Save failed: {err}");
            }
        }
        cx.notify();
    }

    fn export_active_worksheet_markdown(&mut self, cx: &mut Context<Self>) {
        let Some(ws) = self.current_tab().worksheet() else {
            return;
        };
        let mut out = format!("# {}\n\n", self.current_tab().title);
        for cell in &ws.cells {
            out.push_str(&format!("[> {}\n", cell.input));
            let formatted = cell.formatted_output();
            if !formatted.is_empty() {
                out.push_str(&format!("   {}   ({})\n", formatted, cell.index));
            }
            out.push('\n');
        }
        write_clipboard(&out, cx);
        self.status_message = "Exported worksheet Markdown to clipboard".to_string();
        cx.notify();
    }

    fn export_active_cell_latex(&mut self, cx: &mut Context<Self>) {
        let Some(ws) = self.current_tab().worksheet() else {
            return;
        };
        let cell = self
            .current_tab()
            .selected_cell
            .and_then(|i| ws.cells.get(i))
            .or_else(|| ws.cells.last());
        let Some(cell) = cell else {
            self.status_message = "No cell selected to export".to_string();
            cx.notify();
            return;
        };
        if let Some(expr) = &cell.result_expr {
            let latex = to_latex(expr, cell.assigned_name.as_deref());
            write_clipboard(&latex, cx);
            self.status_message = format!("Copied LaTeX ({}) to clipboard", cell.index);
        } else {
            self.status_message = "Selected cell has no symbolic expression".to_string();
        }
        cx.notify();
    }

    fn delete_cell_at(&mut self, cell_idx: usize, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        tab.push_undo(true);
        let Some(ws) = tab.worksheet_mut() else {
            return;
        };
        ws.delete_cell(cell_idx);
        let len = ws.cells.len();
        tab.selected_cell = if len == 0 {
            None
        } else {
            Some(cell_idx.min(len - 1))
        };
        self.status_message = "Deleted cell".to_string();
        cx.notify();
    }

    fn clear_all_outputs(&mut self, cx: &mut Context<Self>) {
        let tab = self.current_tab_mut();
        tab.push_undo(true);
        if let Some(ws) = tab.worksheet_mut() {
            ws.clear_outputs();
            tab.last_elapsed_us = None;
            self.status_message = "Cleared all outputs".to_string();
            cx.notify();
        }
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

    fn unassign_selected_variable(&mut self, cx: &mut Context<Self>) {
        let Some(var) = self.selected_var.clone() else {
            self.status_message = "Select a variable in the table first".to_string();
            cx.notify();
            return;
        };
        if let Some(ws) = self.current_tab_mut().worksheet_mut() {
            ws.env.variables.remove(&var);
            ws.env.functions.remove(&var);
            self.selected_var = None;
            self.status_message = format!("Unassigned `{var}`");
            cx.notify();
        }
    }

    fn step_tab(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.tabs.len() as isize;
        if len == 0 {
            return;
        }
        let next = ((self.active_tab as isize + delta).rem_euclid(len)) as usize;
        self.active_tab = next;
        self.status_message = format!("Switched to {}", self.current_tab().title);
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn render_quick_access_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode_label = match self.entry_mode {
            EntryMode::Math => "Math",
            EntryMode::Text => "Text",
        };
        let can_undo = !self.current_tab().undo_stack.is_empty();
        let can_redo = !self.current_tab().redo_stack.is_empty();
        let max_icon = if window.is_maximized() { "❐" } else { "□" };

        div()
            .w_full()
            .h(px(34.))
            .px_2p5()
            .bg(rgb(0xdce5ef))
            .border_b_1()
            .border_color(rgb(0xc6d2df))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(qa_btn(
                        "qa-new",
                        "[> New",
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.open_starter_worksheet(None, window, cx);
                        }),
                    ))
                    .child(qa_btn(
                        "qa-save",
                        "Save",
                        true,
                        cx.listener(|this, _, _window, cx| {
                            this.save_active_worksheet(cx);
                        }),
                    ))
                    .child(div().w(px(1.)).h(px(16.)).bg(rgb(0xb8c5d4)))
                    .child(qa_btn(
                        "qa-copy",
                        "Copy",
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.copy_action(&CopyAction, window, cx);
                        }),
                    ))
                    .child(qa_btn(
                        "qa-paste",
                        "Paste",
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.paste_action(&PasteAction, window, cx);
                        }),
                    ))
                    .child(div().w(px(1.)).h(px(16.)).bg(rgb(0xb8c5d4)))
                    .child(qa_btn(
                        "qa-undo",
                        "Undo",
                        can_undo,
                        cx.listener(|this, _, window, cx| {
                            this.undo_action(&UndoAction, window, cx);
                        }),
                    ))
                    .child(qa_btn(
                        "qa-redo",
                        "Redo",
                        can_redo,
                        cx.listener(|this, _, window, cx| {
                            this.redo_action(&RedoAction, window, cx);
                        }),
                    ))
                    .child(div().w(px(1.)).h(px(16.)).bg(rgb(0xb8c5d4)))
                    .child(qa_btn(
                        "qa-mode",
                        if mode_label == "Math" {
                            "Mode: Math"
                        } else {
                            "Mode: Text"
                        },
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.toggle_math_text_mode(&ToggleMathTextMode, window, cx);
                        }),
                    ))
                    .child(qa_btn(
                        "qa-exec-all",
                        "!!! Run All",
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.execute_worksheet(&ExecuteWorksheet, window, cx);
                        }),
                    ))
                    .child(qa_btn(
                        "qa-exec-one",
                        "! Run Line",
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.evaluate_input(&Evaluate, window, cx);
                        }),
                    ))
                    .child(div().w(px(1.)).h(px(16.)).bg(rgb(0xb8c5d4)))
                    .child(qa_btn(
                        "qa-zoom-out",
                        "-",
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.zoom_out(&ZoomOut, window, cx);
                        }),
                    ))
                    .child(
                        div()
                            .id("qa-zoom-reset")
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .hover(|s| s.bg(rgb(0xcbd7e6)))
                            .cursor_pointer()
                            .tooltip(make_tooltip("Reset worksheet zoom to 100% (Ctrl+0)"))
                            .text_xs()
                            .text_color(rgb(0x1e293b))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.zoom_reset(&ZoomReset, window, cx);
                            }))
                            .child(format!("{}%", self.zoom_percent)),
                    )
                    .child(qa_btn(
                        "qa-zoom-in",
                        "+",
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.zoom_in(&ZoomIn, window, cx);
                        }),
                    )),
            )
            .child(
                div()
                    .id("window-drag-strip")
                    .flex_1()
                    .h_full()
                    .min_w(px(64.))
                    .px_3()
                    .window_control_area(WindowControlArea::Drag)
                    .cursor(CursorStyle::OpenHand)
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(MouseButton::Left, |ev, window, _| {
                        if ev.click_count >= 2 {
                            window.zoom_window();
                        } else {
                            window.start_window_move();
                        }
                    })
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0x334155))
                            .child(format!("{} — Maple Worksheet", self.current_tab().title)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id("win-minimize")
                            .w(px(28.))
                            .h(px(24.))
                            .rounded_sm()
                            .window_control_area(WindowControlArea::Min)
                            .hover(|s| s.bg(rgb(0xcbd7e6)))
                            .cursor_pointer()
                            .tooltip(make_tooltip("Minimize window"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x1e293b))
                            .on_click(|_, window, _| {
                                window.minimize_window();
                            })
                            .child("─"),
                    )
                    .child(
                        div()
                            .id("win-maximize")
                            .w(px(28.))
                            .h(px(24.))
                            .rounded_sm()
                            .window_control_area(WindowControlArea::Max)
                            .hover(|s| s.bg(rgb(0xcbd7e6)))
                            .cursor_pointer()
                            .tooltip(make_tooltip("Maximize / Restore window"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x1e293b))
                            .on_click(|_, window, _| {
                                window.zoom_window();
                            })
                            .child(max_icon),
                    )
                    .child(
                        div()
                            .id("win-close")
                            .w(px(28.))
                            .h(px(24.))
                            .rounded_sm()
                            .window_control_area(WindowControlArea::Close)
                            .hover(|s| s.bg(rgb(0xdc2626)).text_color(rgb(0xffffff)))
                            .cursor_pointer()
                            .tooltip(make_tooltip("Close Maple Worksheet (Ctrl+Q)"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x1e293b))
                            .on_click(|_, window, _| {
                                window.remove_window();
                            })
                            .child("×"),
                    ),
            )
    }

    fn render_menu_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .h(px(28.))
            .px_3()
            .bg(rgb(0xeaf0f6))
            .border_b_1()
            .border_color(rgb(0xd4dde7))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(div().flex().flex_row().items_center().gap_0p5().children(
                MenuTab::ALL.into_iter().enumerate().map(|(idx, tab)| {
                    let active = self.active_menu == tab;
                    div()
                        .id(("menu", idx))
                        .px_2p5()
                        .py_1()
                        .rounded_t_sm()
                        .bg(if active { rgb(0xf4f7fb) } else { rgb(0xeaf0f6) })
                        .border_b_2()
                        .border_color(if active { rgb(0x1d4ed8) } else { rgb(0xeaf0f6) })
                        .text_xs()
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if active { rgb(0x1d4ed8) } else { rgb(0x1e293b) })
                        .hover(|s| s.bg(rgb(0xf4f7fb)))
                        .cursor_pointer()
                        .tooltip(make_tooltip(format!(
                            "Open {} ribbon commands",
                            tab.label()
                        )))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.active_menu = tab;
                            window.focus(&this.focus_handle);
                            cx.notify();
                        }))
                        .child(tab.label())
                }),
            ))
            .child(
                div()
                    .id("menu-drag-strip")
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag)
                    .cursor(CursorStyle::OpenHand)
                    .on_mouse_down(MouseButton::Left, |ev, window, _| {
                        if ev.click_count >= 2 {
                            window.zoom_window();
                        } else {
                            window.start_window_move();
                        }
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .id("toggle-left-dock")
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xcbd5e1))
                            .text_xs()
                            .text_color(rgb(0x334155))
                            .cursor_pointer()
                            .tooltip(make_tooltip("Show or hide the Palettes & Workbook dock"))
                            .hover(|s| s.bg(rgb(0xf1f5f9)))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.left_dock_open = !this.left_dock_open;
                                cx.notify();
                            }))
                            .child(if self.left_dock_open {
                                "Hide Palettes"
                            } else {
                                "Show Palettes"
                            }),
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
                            .tooltip(make_tooltip("Show or hide the Context Panel dock"))
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
        let base = div()
            .w_full()
            .px_3()
            .py_2()
            .bg(rgb(0xf4f7fb))
            .border_b_1()
            .border_color(rgb(0xd4dde7))
            .flex()
            .flex_row()
            .items_center()
            .flex_wrap()
            .gap_2();

        match self.active_menu {
            MenuTab::Home => self.render_home_ribbon(base, cx),
            MenuTab::File => self.render_file_ribbon(base, cx),
            MenuTab::Insert => self.render_insert_ribbon(base, cx),
            MenuTab::Edit => self.render_edit_ribbon(base, cx),
            MenuTab::View => self.render_view_ribbon(base, cx),
            MenuTab::Evaluation => self.render_evaluation_ribbon(base, cx),
            MenuTab::Tools => self.render_tools_ribbon(base, cx),
            MenuTab::Education => self.render_education_ribbon(base, cx),
            MenuTab::Workbook => self.render_workbook_ribbon(base, cx),
            MenuTab::Help => self.render_help_ribbon(base, cx),
        }
    }

    fn render_home_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        row.child(ribbon_btn(
            "hom-prev",
            "←",
            "Prev Tab",
            rgb(0x1d4ed8),
            cx.listener(|this, _, window, cx| this.step_tab(-1, window, cx)),
        ))
        .child(ribbon_btn(
            "hom-start",
            "⌂",
            "Start.mw",
            rgb(0x1d4ed8),
            cx.listener(|this, _, window, cx| {
                this.active_tab = 0;
                window.focus(&this.focus_handle);
                cx.notify();
            }),
        ))
        .child(ribbon_btn(
            "hom-next",
            "→",
            "Next Tab",
            rgb(0x1d4ed8),
            cx.listener(|this, _, window, cx| this.step_tab(1, window, cx)),
        ))
        .child(ribbon_sep())
        .child(ribbon_btn(
            "hom-copy",
            "⧉",
            "Copy",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.copy_action(&CopyAction, window, cx)),
        ))
        .child(ribbon_btn(
            "hom-paste",
            "📋",
            "Paste",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.paste_action(&PasteAction, window, cx)),
        ))
        .child(ribbon_btn(
            "hom-cut",
            "✂",
            "Cut",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.cut_action(&CutAction, window, cx)),
        ))
        .child(ribbon_sep())
        .child(ribbon_btn(
            "hom-undo",
            "↶",
            "Undo",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.undo_action(&UndoAction, window, cx)),
        ))
        .child(ribbon_btn(
            "hom-redo",
            "↷",
            "Redo",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.redo_action(&RedoAction, window, cx)),
        ))
        .child(ribbon_btn(
            "hom-clear-prompt",
            "⌫",
            "Clear Prompt",
            rgb(0x475569),
            cx.listener(|this, _, window, cx| {
                let tab = this.current_tab_mut();
                tab.push_undo(false);
                tab.input.clear();
                tab.cursor = 0;
                window.focus(&this.focus_handle);
                cx.notify();
            }),
        ))
        .child(ribbon_sep())
        .child(ribbon_btn(
            "hom-exec-ws",
            "[!!!]",
            "Execute Worksheet",
            rgb(0x1d4ed8),
            cx.listener(|this, _, window, cx| {
                this.execute_worksheet(&ExecuteWorksheet, window, cx);
            }),
        ))
        .child(ribbon_btn(
            "hom-exec-sel",
            "[!]",
            "Execute Selection",
            rgb(0x2563eb),
            cx.listener(|this, _, window, cx| {
                this.evaluate_input(&Evaluate, window, cx);
            }),
        ))
        .child(ribbon_btn(
            "hom-exec-code",
            "|>",
            "Execute Code",
            rgb(0x059669),
            cx.listener(|this, _, window, cx| {
                this.evaluate_input(&Evaluate, window, cx);
            }),
        ))
        .child(ribbon_btn(
            "hom-restart",
            "↻",
            "Restart Kernel",
            rgb(0xb91c1c),
            cx.listener(|this, _, window, cx| {
                this.restart_kernel(&RestartKernel, window, cx);
            }),
        ))
        .child(ribbon_btn(
            "hom-startup",
            "{/}",
            "Startup Code",
            rgb(0x0f766e),
            cx.listener(|this, _, window, cx| {
                this.run_statement_in_active_tab("f := x^3 * sin(x);", false, cx);
                window.focus(&this.focus_handle);
            }),
        ))
    }

    fn render_file_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        row.child(ribbon_btn(
            "file-new",
            "[>",
            "New Worksheet (Ctrl+N)",
            rgb(0x1d4ed8),
            cx.listener(|this, _, window, cx| this.open_starter_worksheet(None, window, cx)),
        ))
        .child(ribbon_btn(
            "file-open",
            "📂",
            "Open Saved (.mpl / .mw)",
            rgb(0x2563eb),
            cx.listener(|this, _, window, cx| this.open_saved_worksheet(window, cx)),
        ))
        .child(ribbon_btn(
            "file-sample",
            "+",
            "Sample Session",
            rgb(0x059669),
            cx.listener(|this, _, window, cx| {
                this.open_starter_worksheet(Some("calculus"), window, cx);
            }),
        ))
        .child(ribbon_btn(
            "file-save",
            "💾",
            "Save (.mpl)",
            rgb(0x0f766e),
            cx.listener(|this, _, _window, cx| this.save_active_worksheet(cx)),
        ))
        .child(ribbon_btn(
            "file-md",
            "MD",
            "Export Markdown",
            rgb(0x334155),
            cx.listener(|this, _, _window, cx| this.export_active_worksheet_markdown(cx)),
        ))
        .child(ribbon_btn(
            "file-tex",
            "TeX",
            "Export LaTeX",
            rgb(0x7c3aed),
            cx.listener(|this, _, _window, cx| this.export_active_cell_latex(cx)),
        ))
        .child(ribbon_btn(
            "file-close",
            "×",
            "Close Tab (Ctrl+W)",
            rgb(0x475569),
            cx.listener(|this, _, window, cx| this.close_tab_action(&CloseTabAction, window, cx)),
        ))
        .child(ribbon_btn(
            "file-quit",
            "⏻",
            "Quit",
            rgb(0xb91c1c),
            cx.listener(|_, _, _, cx| cx.quit()),
        ))
    }

    fn render_insert_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        let snippets = [
            ("ins-prompt", "[>", "Prompt", ""),
            (
                "ins-plot",
                "📈",
                "2D Plot",
                "plot([sin(x), cos(x)], x = -Pi..Pi);",
            ),
            ("ins-mat2", "⊞", "2x2 Identity", "Matrix([[1, 0], [0, 1]]);"),
            (
                "ins-mat3",
                "⊞",
                "3x3 Symbolic",
                "Matrix([[a11, a12, a13], [a21, a22, a23], [a31, a32, a33]]);",
            ),
            ("ins-diff", "d/dx", "Derivative", "diff(f, x);"),
            ("ins-int", "∫", "Indefinite Int", "int(f, x);"),
            ("ins-defint", "∫ₐᵇ", "Definite Int", "int(x^2, x = 0..3);"),
            ("ins-lim", "lim", "Limit (x->0)", "limit(sin(x)/x, x = 0);"),
            ("ins-ser", "Σ", "Taylor Series", "series(exp(x), x = 0, 5);"),
        ];
        let mut r = row;
        for (id, icon, label, cmd) in snippets {
            r = r.child(ribbon_btn(
                id,
                icon,
                label,
                rgb(0x1d4ed8),
                cx.listener(move |this, _, window, cx| {
                    if cmd.is_empty() {
                        this.ensure_worksheet_tab();
                        window.focus(&this.focus_handle);
                        cx.notify();
                    } else {
                        this.insert_text(cmd, cx);
                        window.focus(&this.focus_handle);
                    }
                }),
            ));
        }
        r
    }

    fn render_edit_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        row.child(ribbon_btn(
            "ed-undo",
            "↶",
            "Undo (Ctrl+Z)",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.undo_action(&UndoAction, window, cx)),
        ))
        .child(ribbon_btn(
            "ed-redo",
            "↷",
            "Redo (Ctrl+Y)",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.redo_action(&RedoAction, window, cx)),
        ))
        .child(ribbon_sep())
        .child(ribbon_btn(
            "ed-cut",
            "✂",
            "Cut (Ctrl+X)",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.cut_action(&CutAction, window, cx)),
        ))
        .child(ribbon_btn(
            "ed-copy-in",
            "⧉",
            "Copy Input",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| {
                if !this.current_tab().input.is_empty() {
                    let t = this.current_tab().input.clone();
                    write_clipboard(&t, cx);
                    this.status_message = "Copied prompt input".to_string();
                    cx.notify();
                } else if let Some(ws) = this.current_tab().worksheet()
                    && let Some(cell) = this
                        .current_tab()
                        .selected_cell
                        .and_then(|i| ws.cells.get(i))
                        .or_else(|| ws.cells.last())
                {
                    let t = cell.input.clone();
                    write_clipboard(&t, cx);
                    this.status_message = format!("Copied input ({})", cell.index);
                    cx.notify();
                }
                window.focus(&this.focus_handle);
            }),
        ))
        .child(ribbon_btn(
            "ed-copy-out",
            "⧉",
            "Copy Output",
            rgb(0x1d4ed8),
            cx.listener(|this, _, _window, cx| this.copy_selected_or_last_output(cx)),
        ))
        .child(ribbon_btn(
            "ed-copy-tex",
            "TeX",
            "Copy as LaTeX",
            rgb(0x7c3aed),
            cx.listener(|this, _, _window, cx| this.export_active_cell_latex(cx)),
        ))
        .child(ribbon_btn(
            "ed-paste",
            "📋",
            "Paste (Ctrl+V)",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.paste_action(&PasteAction, window, cx)),
        ))
        .child(ribbon_sep())
        .child(ribbon_btn(
            "ed-del-cell",
            "🗑",
            "Delete Cell",
            rgb(0xb91c1c),
            cx.listener(|this, _, _window, cx| {
                if let Some(sel) = this.current_tab().selected_cell {
                    this.delete_cell_at(sel, cx);
                }
            }),
        ))
    }

    fn render_view_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        row.child(ribbon_btn(
            "vw-left",
            "◧",
            "Toggle Palettes",
            rgb(0x1d4ed8),
            cx.listener(|this, _, _window, cx| {
                this.left_dock_open = !this.left_dock_open;
                cx.notify();
            }),
        ))
        .child(ribbon_btn(
            "vw-right",
            "◨",
            "Toggle Context Panel",
            rgb(0x1d4ed8),
            cx.listener(|this, _, _window, cx| {
                this.right_dock_open = !this.right_dock_open;
                cx.notify();
            }),
        ))
        .child(ribbon_sep())
        .child(ribbon_btn(
            "vw-zin",
            "+",
            "Zoom In",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.zoom_in(&ZoomIn, window, cx)),
        ))
        .child(ribbon_btn(
            "vw-zout",
            "-",
            "Zoom Out",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.zoom_out(&ZoomOut, window, cx)),
        ))
        .child(ribbon_btn(
            "vw-zrst",
            "100%",
            "Reset Zoom",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.zoom_reset(&ZoomReset, window, cx)),
        ))
        .child(ribbon_sep())
        .child(ribbon_btn(
            "vw-exp-all",
            "▾",
            "Expand Palettes",
            rgb(0x0f766e),
            cx.listener(|this, _, _window, cx| {
                this.palettes = PaletteExpansion {
                    favorites: true,
                    expression: true,
                    calculus: true,
                    variables: true,
                    matrix: true,
                    symbols: true,
                };
                cx.notify();
            }),
        ))
        .child(ribbon_btn(
            "vw-col-all",
            "▸",
            "Collapse Palettes",
            rgb(0x475569),
            cx.listener(|this, _, _window, cx| {
                this.palettes = PaletteExpansion {
                    favorites: false,
                    expression: false,
                    calculus: false,
                    variables: false,
                    matrix: false,
                    symbols: false,
                };
                cx.notify();
            }),
        ))
    }

    fn render_evaluation_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        row.child(ribbon_btn(
            "ev-all",
            "[!!!]",
            "Execute Worksheet",
            rgb(0x1d4ed8),
            cx.listener(|this, _, window, cx| {
                this.execute_worksheet(&ExecuteWorksheet, window, cx);
            }),
        ))
        .child(ribbon_btn(
            "ev-sel",
            "[!]",
            "Execute Selection",
            rgb(0x2563eb),
            cx.listener(|this, _, window, cx| this.evaluate_input(&Evaluate, window, cx)),
        ))
        .child(ribbon_btn(
            "ev-rst",
            "↻",
            "Restart Kernel",
            rgb(0xb91c1c),
            cx.listener(|this, _, window, cx| this.restart_kernel(&RestartKernel, window, cx)),
        ))
        .child(ribbon_btn(
            "ev-clr-out",
            "⌫",
            "Clear Outputs",
            rgb(0x475569),
            cx.listener(|this, _, _window, cx| this.clear_all_outputs(cx)),
        ))
        .child(ribbon_sep())
        .child(ribbon_btn(
            "ev-simp",
            "≈",
            "Simplify %",
            rgb(0x059669),
            cx.listener(|this, _, window, cx| {
                this.run_context_action("simplify(%);", window, cx);
            }),
        ))
        .child(ribbon_btn(
            "ev-exp",
            "⇔",
            "Expand %",
            rgb(0x059669),
            cx.listener(|this, _, window, cx| {
                this.run_context_action("expand(%);", window, cx);
            }),
        ))
        .child(ribbon_btn(
            "ev-fac",
            "∏",
            "Factor %",
            rgb(0x059669),
            cx.listener(|this, _, window, cx| {
                this.run_context_action("factor(%);", window, cx);
            }),
        ))
        .child(ribbon_btn(
            "ev-num",
            "1.0",
            "evalf(%)",
            rgb(0x059669),
            cx.listener(|this, _, window, cx| {
                this.run_context_action("evalf(%);", window, cx);
            }),
        ))
        .child(ribbon_btn(
            "ev-solve",
            "=",
            "Solve % = 0",
            rgb(0x7c3aed),
            cx.listener(|this, _, window, cx| {
                this.run_context_action("solve(% = 0, x);", window, cx);
            }),
        ))
    }

    fn render_tools_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        let tools = [
            (
                "tl-plot",
                "📈",
                "Plot Builder",
                "plot([sin(x), cos(x), x/3], x = -Pi..Pi);",
            ),
            (
                "tl-rref",
                "⊞",
                "RREF Solver",
                "rref(Matrix([[1, 2, -1, -4], [2, 3, -1, -11], [-2, 0, -3, 22]]));",
            ),
            (
                "tl-eig",
                "λ",
                "Eigenvalue Finder",
                "Eigenvalues(Matrix([[4, 1], [2, 3]]));",
            ),
            ("tl-tay", "Σ", "Taylor Series", "series(exp(x), x = 0, 5);"),
            ("tl-evalf", "π", "Numeric Pi", "evalf(Pi);"),
        ];
        let mut r = row;
        for (id, icon, label, cmd) in tools {
            r = r.child(ribbon_btn(
                id,
                icon,
                label,
                rgb(0x1d4ed8),
                cx.listener(move |this, _, window, cx| {
                    this.run_statement_in_active_tab(cmd, false, cx);
                    window.focus(&this.focus_handle);
                }),
            ));
        }
        r
    }

    fn render_education_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        row.child(ribbon_btn(
            "edu-guides",
            "📘",
            "Study Guides",
            rgb(0x1d4ed8),
            cx.listener(|this, _, window, cx| this.open_study_guide_tab(0, window, cx)),
        ))
        .child(ribbon_btn(
            "edu-precalc",
            "|x|",
            "Precalculus (Inequalities)",
            rgb(0x0f766e),
            cx.listener(|this, _, window, cx| this.open_study_guide_tab(0, window, cx)),
        ))
        .child(ribbon_btn(
            "edu-deplot",
            "dy/dx",
            "DEtools[DEplot] Tutor",
            rgb(0x7c3aed),
            cx.listener(|this, _, window, cx| this.open_deplot_tutor_tab(window, cx)),
        ))
        .child(ribbon_btn(
            "edu-calc1",
            "∫x",
            "Calculus — Single Variable",
            rgb(0x059669),
            cx.listener(|this, _, window, cx| this.open_study_guide_tab(1, window, cx)),
        ))
        .child(ribbon_btn(
            "edu-calc3d",
            "🧊",
            "3D Surface Plot Demo",
            rgb(0x2563eb),
            cx.listener(|this, _, window, cx| {
                this.run_statement_in_active_tab(
                    "plot3d(sin(x)*cos(y), x = -Pi..Pi, y = -Pi..Pi);",
                    false,
                    cx,
                );
                window.focus(&this.focus_handle);
            }),
        ))
        .child(ribbon_btn(
            "edu-dsolve",
            "y''",
            "ODE dsolve Demo",
            rgb(0xb91c1c),
            cx.listener(|this, _, window, cx| {
                this.run_statement_in_active_tab("dsolve(diff(y, x$2) + y = 0, y);", false, cx);
                window.focus(&this.focus_handle);
            }),
        ))
        .child(ribbon_btn(
            "edu-help",
            "🎓",
            "Student Help Center",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| this.open_help_browser_tab(0, window, cx)),
        ))
    }

    fn render_workbook_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        row.child(ribbon_btn(
            "wb-new-rib",
            "[>",
            "New Empty Worksheet",
            rgb(0x1d4ed8),
            cx.listener(|this, _, window, cx| this.open_starter_worksheet(None, window, cx)),
        ))
        .child(ribbon_btn(
            "wb-dup",
            "⧉",
            "Duplicate Worksheet",
            rgb(0x059669),
            cx.listener(|this, _, window, cx| this.duplicate_active_worksheet(window, cx)),
        ))
        .child(ribbon_btn(
            "wb-start",
            "⌂",
            "Switch to Start.mw",
            rgb(0x334155),
            cx.listener(|this, _, window, cx| {
                this.active_tab = 0;
                window.focus(&this.focus_handle);
                cx.notify();
            }),
        ))
        .child(ribbon_btn(
            "wb-close",
            "×",
            "Close Active Worksheet",
            rgb(0xb91c1c),
            cx.listener(|this, _, window, cx| this.close_tab_action(&CloseTabAction, window, cx)),
        ))
    }

    fn render_help_ribbon(&self, row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        row.child(ribbon_btn(
            "hlp-browser",
            "📖",
            "Maple 2025 Help",
            rgb(0x1d4ed8),
            cx.listener(|this, _, window, cx| this.open_help_browser_tab(0, window, cx)),
        ))
        .child(ribbon_btn(
            "hlp-guides",
            "📘",
            "Study Guides Reference",
            rgb(0x0f766e),
            cx.listener(|this, _, window, cx| this.open_study_guide_tab(0, window, cx)),
        ))
        .child(ribbon_btn(
            "hlp-tour",
            "?",
            "Open Sample Tour",
            rgb(0x2563eb),
            cx.listener(|this, _, window, cx| {
                this.open_starter_worksheet(Some("calculus"), window, cx);
            }),
        ))
        .child(ribbon_btn(
            "hlp-syntax",
            "λ",
            "Insert Arrow Fn Example",
            rgb(0x0f766e),
            cx.listener(|this, _, window, cx| {
                this.set_input_and_focus("g := x -> x^2 + 2*x + 1;", window, cx);
            }),
        ))
        .child(ribbon_btn(
            "hlp-reset",
            "↻",
            "Reset Kernel",
            rgb(0xb91c1c),
            cx.listener(|this, _, window, cx| this.restart_kernel(&RestartKernel, window, cx)),
        ))
    }

    fn render_left_dock(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(274.))
            .h_full()
            .min_h_0()
            .bg(rgb(0xf8fafc))
            .border_r_1()
            .border_color(rgb(0xd4dde7))
            .flex()
            .flex_col()
            .child(
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
                    .w_full()
                    .overflow_y_scroll()
                    .child(div().w_full().p_2().pb_6().flex().flex_col().gap_2().child(
                        match self.left_dock_tab {
                            LeftDockTab::Palettes => {
                                self.render_palettes_pane(cx).into_any_element()
                            }
                            LeftDockTab::Workbook => {
                                self.render_workbook_pane(cx).into_any_element()
                            }
                        },
                    )),
            )
    }

    fn render_palettes_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let fav_items = [
            "diff(f, x);",
            "int(f, x);",
            "solve(f = 0, x);",
            "rref(A);",
            "plot(f, x = -Pi..Pi);",
        ];
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
            // Favorites
            .child(accordion_header(
                "pal-fav",
                "Favorites",
                self.palettes.favorites,
                cx.listener(|this, _, _window, cx| {
                    this.palettes.favorites = !this.palettes.favorites;
                    cx.notify();
                }),
            ))
            .when(self.palettes.favorites, |el| {
                el.child(div().px_1().pb_1().flex().flex_col().gap_1().children(
                    fav_items.into_iter().enumerate().map(|(i, cmd)| {
                        div()
                            .id(("fav-tmpl", i))
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
            // Expression
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
            // Calculus
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
            // Variables table + 3 action buttons matching Maple 2025 screenshot
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
                                    let is_sel = self.selected_var.as_deref() == Some(k.as_str());
                                    let k_select = k.clone();
                                    div()
                                        .id(("var-row", idx))
                                        .px_2()
                                        .py_1()
                                        .bg(if is_sel { rgb(0xdbeafe) } else { rgb(0xffffff) })
                                        .border_t_1()
                                        .border_color(rgb(0xf1f5f9))
                                        .hover(|s| s.bg(rgb(0xeff6ff)))
                                        .cursor_pointer()
                                        .flex()
                                        .flex_row()
                                        .justify_between()
                                        .gap_2()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.selected_var = Some(k_select.clone());
                                            this.insert_text(&k_select, cx);
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
                        })
                        .child(
                            div()
                                .px_1p5()
                                .py_1()
                                .bg(rgb(0xf8fafc))
                                .border_t_1()
                                .border_color(rgb(0xe2e8f0))
                                .flex()
                                .flex_row()
                                .justify_between()
                                .gap_1()
                                .child(small_action_btn(
                                    "var-ins",
                                    "Insert",
                                    cx.listener(|this, _, window, cx| {
                                        if let Some(var) = this.selected_var.clone() {
                                            this.insert_text(&var, cx);
                                            window.focus(&this.focus_handle);
                                        }
                                    }),
                                ))
                                .child(small_action_btn(
                                    "var-unassign",
                                    "Unassign",
                                    cx.listener(|this, _, _window, cx| {
                                        this.unassign_selected_variable(cx);
                                    }),
                                ))
                                .child(small_action_btn(
                                    "var-clear",
                                    "Clear All",
                                    cx.listener(|this, _, window, cx| {
                                        this.restart_kernel(&RestartKernel, window, cx);
                                    }),
                                )),
                        ),
                )
            })
            // Matrix Palette
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

        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_1()
            .child(
                div()
                    .rounded_md()
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0xd4dde7))
                    .p_2p5()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x334155))
                            .child("▾ Navigator"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x475569))
                            .child("This file is not a workbook. Save the file as a workbook in order to use the navigator."),
                    )
                    .child(
                        div()
                            .id("wb-save-as")
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(rgb(0xeaf0f6))
                            .border_1()
                            .border_color(rgb(0xcbd5e1))
                            .hover(|s| s.bg(rgb(0xdbeafe)).border_color(rgb(0x93c5fd)))
                            .cursor_pointer()
                            .tooltip(make_tooltip("Save active worksheet script into ./worksheets"))
                            .flex()
                            .justify_center()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(0x0f172a))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.save_active_worksheet(cx);
                            }))
                            .child("Save As Workbook"),
                    ),
            )
            .child(
                div()
                    .rounded_md()
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0xd4dde7))
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x334155))
                            .child("▾ Variables"),
                    )
                    .child(
                        div()
                            .px_1()
                            .py_0p5()
                            .border_b_1()
                            .border_color(rgb(0xe2e8f0))
                            .flex()
                            .flex_row()
                            .justify_between()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0x64748b))
                            .child("Variable")
                            .child("Value"),
                    )
                    .when(vars.is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x94a3b8))
                                .child("No variables assigned yet."),
                        )
                    })
                    .children(vars.into_iter().take(12).map(|(k, v)| {
                        div()
                            .px_1()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .gap_2()
                            .text_xs()
                            .child(div().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x1e293b)).child(k))
                            .child(div().text_color(rgb(0x475569)).child(v))
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
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(0xeef2f6))
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
                            .h(px(28.))
                            .rounded_t_md()
                            .bg(if active { rgb(0xffffff) } else { rgb(0xdbe3ec) })
                            .border_t_1()
                            .border_l_1()
                            .border_r_1()
                            .border_color(if active { rgb(0xcbd5e1) } else { rgb(0xd4dde7) })
                            .flex()
                            .flex_row()
                            .items_center()
                            .child(
                                div()
                                    .id(("doc-tab-select", tab.id))
                                    .px_2p5()
                                    .h_full()
                                    .cursor_pointer()
                                    .tooltip(make_tooltip(format!("Switch to {}", tab.title)))
                                    .flex()
                                    .items_center()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if idx < this.tabs.len() {
                                            this.active_tab = idx;
                                        }
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
                                    ),
                            )
                            .when(closable, |el| {
                                el.child(
                                    div()
                                        .id(("doc-close", tab.id))
                                        .mr_1()
                                        .px_1()
                                        .rounded_sm()
                                        .cursor_pointer()
                                        .tooltip(make_tooltip("Close tab (Ctrl+W)"))
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
                            .tooltip(make_tooltip("Open a new worksheet tab (Ctrl+N)"))
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x334155))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_starter_worksheet(None, window, cx);
                            }))
                            .child("+"),
                    ),
            )
            .child(match &self.current_tab().content {
                DocumentContent::StartPage => self.render_start_page(cx).into_any_element(),
                DocumentContent::Worksheet(ws) => {
                    self.render_worksheet_document(ws, cx).into_any_element()
                }
                DocumentContent::DePlotTutor {
                    preset_idx,
                    x0,
                    y0,
                    show_arrows,
                } => self
                    .render_deplot_tutor(*preset_idx, *x0, *y0, *show_arrows, cx)
                    .into_any_element(),
                DocumentContent::StudyGuide { chapter_idx } => {
                    self.render_study_guide(*chapter_idx, cx).into_any_element()
                }
                DocumentContent::HelpBrowser { article_idx } => self
                    .render_help_browser(*article_idx, cx)
                    .into_any_element(),
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
                rgb(0xb91c1c),
            ),
        ];

        div()
            .id("start-page-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .p_6()
                    .pb_12()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(780.))
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
                                    .child("Native symbolic algebra, calculus, exact linear algebra, and 2D vector plotting"),
                            ),
                    )
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
                                                .w(px(344.))
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
                ),
            )
    }

    fn render_worksheet_document(
        &self,
        ws: &Worksheet,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tab = self.current_tab();
        let before = tab.input[..tab.cursor].to_string();
        let after = tab.input[tab.cursor..].to_string();
        let is_empty = tab.input.is_empty();
        let selected = tab.selected_cell;
        let zoom = self.zoom_percent as f32 / 100.0;
        let mut cell_views = Vec::with_capacity(ws.cells.len());
        for (idx, cell) in ws.cells.iter().enumerate() {
            cell_views.push(self.render_execution_group(
                idx,
                cell,
                selected == Some(idx),
                zoom,
                cx,
            ));
        }

        div()
            .id("worksheet-doc-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&tab.scroll_handle)
            .child(
                div()
                    .w_full()
                    .p_5()
                    .pb_12()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(900.))
                            .min_h(px(640.))
                    .rounded_md()
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0xd4dde7))
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .when(ws.cells.is_empty(), |el| {
                        el.child(
                            div()
                                .pb_2()
                                .text_xs()
                                .text_color(rgb(0x64748b))
                                .child("Type a Maple command below (e.g., f := x^3*sin(x); or rref(Matrix([[1,2,3],[4,5,6]]));) and press Enter."),
                        )
                    })
                    .children(cell_views)
                    // Active inline Maple `[>` execution group
                    .child(
                        div()
                            .mt_1()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(6.))
                                    .border_l_2()
                                    .border_t_2()
                                    .border_b_2()
                                    .border_color(rgb(0x991b1b)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .py_1p5()
                                    .px_2()
                                    .bg(rgb(0xfdfdfe))
                                    .rounded_r_sm()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0x991b1b))
                                            .child("[>"),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .text_size(px(14.0 * zoom))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0x991b1b))
                                            .when_some(tab.editing_cell, |el, edit_i| {
                                                el.child(
                                                    div()
                                                        .mr_2()
                                                        .px_1p5()
                                                        .py_0p5()
                                                        .rounded_sm()
                                                        .bg(rgb(0xfef3c7))
                                                        .text_xs()
                                                        .text_color(rgb(0x92400e))
                                                        .child(format!("Editing ({})", edit_i + 1)),
                                                )
                                            })
                                            .child(before)
                                            .child(div().w(px(2.)).h(px(17. * zoom)).bg(rgb(0x991b1b)))
                                            .child(if is_empty {
                                                div()
                                                    .pl_2()
                                                    .font_weight(FontWeight::NORMAL)
                                                    .text_color(rgb(0x94a3b8))
                                                    .child("Enter Maple expression...")
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
                                            .tooltip(make_tooltip("Evaluate current prompt and advance (Enter)"))
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0xffffff))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.evaluate_input(&Evaluate, window, cx);
                                            }))
                                            .child("Run ↵"),
                                    ),
                            ),
                    ),
            ),
            )
    }

    fn render_execution_group(
        &self,
        idx: usize,
        cell: &WorksheetCell,
        is_selected: bool,
        zoom: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let cell_input = cell.input.clone();
        let edit_cell_input = cell.input.clone();
        let copy_in_str = cell.input.clone();
        let copy_out_str = cell_output_clipboard_text(cell);
        let latex_str = cell
            .result_expr
            .as_ref()
            .map(|e| to_latex(e, cell.assigned_name.as_deref()));
        let bracket_color = if is_selected {
            rgb(0x2563eb)
        } else {
            rgb(0x64748b)
        };

        div()
            .id(("cell", idx))
            .w_full()
            .py_1()
            .px_1p5()
            .rounded_sm()
            .bg(if is_selected {
                rgb(0xf8fbff)
            } else {
                rgb(0xffffff)
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
            .flex_row()
            .gap_2()
            // Maple 2025 left execution-group bracket `[`
            .child(
                div()
                    .w(px(6.))
                    .border_l_2()
                    .border_t_2()
                    .border_b_2()
                    .border_color(bracket_color),
            )
            // Input + centered 2D Output
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    // Crimson `[>` input line
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
                                            .text_size(px(14.0 * zoom))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0x991b1b))
                                            .child("[>"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.0 * zoom))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0x991b1b))
                                            .child(cell.input.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(cell_pill(
                                        ("edit-cell", idx),
                                        "Edit",
                                        cx.listener(move |this, _, window, cx| {
                                            let t = this.current_tab_mut();
                                            t.editing_cell = Some(idx);
                                            t.selected_cell = Some(idx);
                                            t.input = edit_cell_input.clone();
                                            t.cursor = t.input.len();
                                            this.status_message = format!(
                                                "Editing cell {} — modify prompt below and press Enter",
                                                idx + 1
                                            );
                                            window.focus(&this.focus_handle);
                                            cx.notify();
                                        }),
                                    ))
                                    .child(cell_pill(
                                        ("cp-in", idx),
                                        "Copy In",
                                        cx.listener(move |this, _, _window, cx| {
                                            write_clipboard(&copy_in_str, cx);
                                            this.status_message = "Copied input".to_string();
                                            cx.notify();
                                        }),
                                    ))
                                    .when_some(copy_out_str, |el, out_str| {
                                        el.child(cell_pill(
                                            ("cp-out", idx),
                                            "Copy Out",
                                            cx.listener(move |this, _, _window, cx| {
                                                write_clipboard(&out_str, cx);
                                                this.status_message = "Copied output".to_string();
                                                cx.notify();
                                            }),
                                        ))
                                    })
                                    .when_some(latex_str, |el, ltx| {
                                        el.child(cell_pill(
                                            ("cp-tex", idx),
                                            "LaTeX",
                                            cx.listener(move |this, _, _window, cx| {
                                                write_clipboard(&ltx, cx);
                                                this.status_message = "Copied LaTeX".to_string();
                                                cx.notify();
                                            }),
                                        ))
                                    })
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(rgb(0xf1f5f9))
                                            .text_xs()
                                            .text_color(rgb(0x64748b))
                                            .child(format!("{} μs", cell.elapsed_us)),
                                    )
                                    .child(cell_pill(
                                        ("del-cell", idx),
                                        "×",
                                        cx.listener(move |this, _, _window, cx| {
                                            this.delete_cell_at(idx, cx);
                                        }),
                                    )),
                            ),
                    )
                    // Centered 2D typeset output + right-aligned bold `(n)` equation number
                    .when(!matches!(cell.output, CellOutput::Suppressed), |el| {
                        el.child(
                            div()
                                .w_full()
                                .py_1()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(div().w(px(28.)))
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .justify_center()
                                        .items_center()
                                        .child(self.render_2d_cell_output(cell, zoom, cx)),
                                )
                                .child(
                                    div()
                                        .w(px(36.))
                                        .flex()
                                        .justify_end()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0x0f172a))
                                        .child(format!("({})", cell.index)),
                                ),
                        )
                    }),
            )
    }

    fn render_right_dock(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_cell = self.current_tab().worksheet().and_then(|ws| {
            let idx = self
                .current_tab()
                .selected_cell
                .filter(|&i| i < ws.cells.len())
                .or_else(|| ws.cells.len().checked_sub(1))?;
            ws.cells.get(idx)
        });

        let preview_card = selected_cell.and_then(|c| {
            let plot = c.preview_plot.as_ref()?.clone();
            let expr_label = c.formatted_output();
            Some((plot, expr_label))
        });

        let context_actions = selected_cell.map(|c| (c.index, c.context_actions()));

        let shortcuts = [
            ("Evaluate", "Enter"),
            ("Execute Worksheet", "Ctrl+Shift+Enter"),
            ("Toggle Math/Text", "F5"),
            ("Copy / Cut / Paste", "Ctrl+C / X / V"),
            ("Undo / Redo", "Ctrl+Z / Y"),
            ("Restart Kernel", "Ctrl+R"),
            ("Assignments", "a := b"),
            ("Functions", "f := x -> x^2"),
        ];

        div()
            .w(px(256.))
            .h_full()
            .min_h_0()
            .bg(rgb(0xf8fafc))
            .border_l_1()
            .border_color(rgb(0xd4dde7))
            .flex()
            .flex_col()
            .child(
                div()
                    .id("right-dock-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .p_2p5()
                            .pb_8()
                            .flex()
                            .flex_col()
                            .gap_3()
                            // Live 2D Plot preview card matching Maple 2025 right dock
            .when_some(preview_card, |el, (plot, expr_label)| {
                el.child(
                    div()
                        .id("ctx-plot-preview")
                        .rounded_md()
                        .bg(rgb(0xffffff))
                        .border_1()
                        .border_color(rgb(0xcbd5e1))
                        .hover(|s| s.border_color(rgb(0x3b82f6)))
                        .cursor_pointer()
                        .p_2()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1p5()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.run_context_action("plot(%, x = -10..10);", window, cx);
                        }))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(0x334155))
                                .child("2D Plot"),
                        )
                        .child(
                            div()
                                .w(px(140.))
                                .h(px(74.))
                                .rounded_sm()
                                .bg(rgb(0xf8fafc))
                                .border_1()
                                .border_color(rgb(0xdbe3ed))
                                .child(
                                    canvas(
                                        move |_bounds, _window, _cx| plot.clone(),
                                        move |bounds, data, window, _cx| {
                                            paint_plot_canvas(
                                                bounds,
                                                &data,
                                                &[rgb(0xb91c1c)],
                                                window,
                                                1.5,
                                                4.0,
                                            );
                                        },
                                    )
                                    .size_full(),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(0x1d4ed8))
                                .child(expr_label),
                        ),
                )
            })
            // Context operations list
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
                            .pb_0p5()
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
                                    .child(format!("Expression ({cell_idx}):")),
                            )
                            .child(
                                div()
                                    .id("ctx-eval-pt")
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgb(0xf8fafc))
                                    .border_1()
                                    .border_color(rgb(0xe2e8f0))
                                    .hover(|s| s.bg(rgb(0xeff6ff)).border_color(rgb(0x93c5fd)))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(rgb(0x1e293b))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.run_context_action("subs(x = 1, %);", window, cx);
                                    }))
                                    .child("Evaluate at a Point (x = 1)")
                                    .child(div().text_color(rgb(0x94a3b8)).child("›")),
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
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(rgb(0x1e293b))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.run_context_action(&cmd, window, cx);
                                    }))
                                    .child(label)
                                    .child(div().text_color(rgb(0x94a3b8)).child("›"))
                            }))
                            .into_any_element(),
                        _ => div()
                            .text_xs()
                            .text_color(rgb(0x94a3b8))
                            .child("Evaluate or select an expression or matrix to access context actions.")
                            .into_any_element(),
                    }),
            )
            // Math Editor Shortcuts
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
                            .pb_0p5()
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
            ),
            ),
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
                    .child(format!("Zoom: {}%", self.zoom_percent))
                    .child(mode_str),
            )
    }
}

impl Render for MapleWorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_action(cx.listener(Self::close_tab_action))
            .on_action(cx.listener(Self::copy_action))
            .on_action(cx.listener(Self::cut_action))
            .on_action(cx.listener(Self::paste_action))
            .on_action(cx.listener(Self::undo_action))
            .on_action(cx.listener(Self::redo_action))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xeef2f6))
            .child(self.render_quick_access_bar(window, cx))
            .child(self.render_menu_strip(cx))
            .child(self.render_ribbon(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_row()
                    .when(self.left_dock_open, |el| {
                        el.child(self.render_left_dock(cx))
                    })
                    .child(self.render_center_workspace(cx))
                    .when(self.right_dock_open, |el| {
                        el.child(self.render_right_dock(cx))
                    }),
            )
            .child(self.render_status_bar())
    }
}

fn write_clipboard(text: &str, cx: &mut Context<MapleWorkspaceView>) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    let owned = text.to_string();
    std::thread::spawn(move || {
        if let Ok(mut child) = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(owned.as_bytes());
            }
            let _ = child.wait();
        }
    });
}

fn read_clipboard(cx: &mut Context<MapleWorkspaceView>) -> Option<String> {
    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
        && !text.is_empty()
    {
        return Some(text);
    }
    let output = Command::new("wl-paste").arg("--no-newline").output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

fn cell_output_clipboard_text(cell: &WorksheetCell) -> Option<String> {
    match &cell.output {
        CellOutput::Text(t) => Some(t.clone()),
        CellOutput::MatrixGrid(rows) => Some(format!(
            "Matrix([{}])",
            rows.iter()
                .map(|r| format!("[{}]", r.join(", ")))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        CellOutput::Plot(p) => Some(p.title.clone()),
        CellOutput::Plot3d(s) => Some(s.title.clone()),
        CellOutput::Suppressed => None,
        CellOutput::Error(e) => Some(e.clone()),
    }
}

struct TooltipView {
    text: String,
}

impl Render for TooltipView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2p5()
            .py_1()
            .rounded_md()
            .bg(rgb(0x0f172a))
            .border_1()
            .border_color(rgb(0x334155))
            .shadow_md()
            .text_xs()
            .text_color(rgb(0xf8fafc))
            .child(self.text.clone())
    }
}

fn make_tooltip(text: impl Into<String>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_window, cx| cx.new(|_| TooltipView { text: text.clone() }).into()
}

fn qa_tooltip(id: &str, _label: &str) -> &'static str {
    match id {
        "qa-new" => "Open a new blank Maple worksheet tab (Ctrl+N)",
        "qa-save" => "Export active worksheet to a .mpl script in ~/Documents",
        "qa-copy" => "Copy current prompt or selected output (Ctrl+C)",
        "qa-paste" => "Paste from clipboard into the active prompt (Ctrl+V)",
        "qa-undo" => "Undo last prompt edit (Ctrl+Z)",
        "qa-redo" => "Redo reverted prompt edit (Ctrl+Y)",
        "qa-mode" => "Switch between 2D Math input and Text mode (F5)",
        "qa-exec-all" => {
            "Re-evaluate every execution group in the active worksheet (Ctrl+Shift+Enter)"
        }
        "qa-exec-one" => "Evaluate the current input line in the active worksheet (Enter)",
        "qa-zoom-out" => "Decrease worksheet text and output magnification (Ctrl+-)",
        "qa-zoom-in" => "Increase worksheet text and output magnification (Ctrl+=)",
        _ => "Worksheet toolbar action",
    }
}

fn ribbon_tooltip(id: &str, label: &str) -> String {
    match id {
        "hom-exec-ws" | "ev-all" => {
            "Execute all worksheet prompts sequentially from top to bottom (Ctrl+Shift+Enter)"
                .to_string()
        }
        "hom-exec-sel" | "ev-sel" => {
            "Evaluate the current input prompt and display typeset output (Enter)".to_string()
        }
        "hom-exec-code" => "Evaluate the active code/math expression now".to_string(),
        "hom-restart" | "ev-rst" => {
            "Clear all kernel variables and re-initialize a fresh session (Ctrl+R)".to_string()
        }
        "hom-startup" => "Define a sample symbolic helper function f := x^3 * sin(x)".to_string(),
        _ => label.to_string(),
    }
}

fn qa_btn(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .tooltip(make_tooltip(qa_tooltip(id, label)))
        .when(enabled, |el| {
            el.hover(|s| s.bg(rgb(0xcbd7e6)))
                .cursor_pointer()
                .on_click(on_click)
        })
        .text_xs()
        .text_color(if enabled {
            rgb(0x1e293b)
        } else {
            rgb(0x94a3b8)
        })
        .child(label)
}

fn ribbon_sep() -> impl IntoElement {
    div().w(px(1.)).h(px(26.)).bg(rgb(0xcbd5e1))
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
        .tooltip(make_tooltip(ribbon_tooltip(id, label)))
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

fn cell_pill(
    id: (&'static str, usize),
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let tip = match label {
        "Copy In" => "Copy this Maple input command to clipboard",
        "Copy Out" => "Copy formatted evaluation output to clipboard",
        "LaTeX" => "Copy LaTeX representation of this result",
        "Edit" => "Edit this historical prompt in place",
        "Delete" => "Delete this execution cell from the worksheet",
        _ => "Cell action",
    };
    div()
        .id(id)
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .bg(rgb(0xf8fafc))
        .border_1()
        .border_color(rgb(0xe2e8f0))
        .hover(|s| s.bg(rgb(0xeff6ff)).border_color(rgb(0x93c5fd)))
        .cursor_pointer()
        .tooltip(make_tooltip(tip))
        .text_xs()
        .text_color(rgb(0x475569))
        .on_click(on_click)
        .child(label)
}

fn small_action_btn(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .py_0p5()
        .rounded_sm()
        .bg(rgb(0xffffff))
        .border_1()
        .border_color(rgb(0xcbd5e1))
        .hover(|s| s.bg(rgb(0xeff6ff)).border_color(rgb(0x93c5fd)))
        .cursor_pointer()
        .flex()
        .justify_center()
        .text_xs()
        .text_color(rgb(0x1e293b))
        .on_click(on_click)
        .child(label)
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

impl MapleWorkspaceView {
    fn render_2d_cell_output(
        &self,
        cell: &WorksheetCell,
        zoom: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match &cell.output {
            CellOutput::Plot(plot) => self
                .render_plot_card(cell.index, plot, cx)
                .into_any_element(),
            CellOutput::Plot3d(surf) => self
                .render_plot3d_card(cell.index, surf, cx)
                .into_any_element(),
            CellOutput::Error(err) => div()
                .p_2()
                .rounded_sm()
                .bg(rgb(0xfef2f2))
                .border_1()
                .border_color(rgb(0xfecaca))
                .text_sm()
                .text_color(rgb(0xb91c1c))
                .child(format!("Error: {err}"))
                .into_any_element(),
            CellOutput::Suppressed => div().into_any_element(),
            CellOutput::MatrixGrid(_) | CellOutput::Text(_) => {
                if let Some(expr) = &cell.result_expr {
                    let node = render_2d_math_node(expr, 0, false, zoom);
                    if let Some(name) = &cell.assigned_name {
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(math_span(format!("{name} :="), zoom))
                            .child(node)
                            .into_any_element()
                    } else {
                        node
                    }
                } else {
                    math_span(cell.formatted_output(), zoom)
                }
            }
        }
    }

    fn render_plot_card(
        &self,
        cell_index: usize,
        plot: &PlotData,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let palette = [rgb(0x2563eb), rgb(0xdc2626), rgb(0x059669), rgb(0xd97706)];
        let tab = self.current_tab();
        let zoom_f = tab.plot2d_zoom.max(0.2);
        let pan_x = tab.plot2d_pan_x;

        let x_mid = 0.5 * (plot.x_min + plot.x_max) + pan_x;
        let half_x = 0.5 * (plot.x_max - plot.x_min) / zoom_f;
        let y_mid = 0.5 * (plot.y_min + plot.y_max);
        let half_y = 0.5 * (plot.y_max - plot.y_min) / zoom_f;

        let mut view_plot = plot.clone();
        view_plot.x_min = x_mid - half_x;
        view_plot.x_max = x_mid + half_x;
        view_plot.y_min = y_mid - half_y;
        view_plot.y_max = y_mid + half_y;
        let plot_data = view_plot.clone();

        div()
            .w_full()
            .max_w(px(640.))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0x334155))
                            .child(plot.title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(cell_pill(
                                ("p2-zin", cell_index),
                                "Zoom +",
                                cx.listener(|this, _, _window, cx| {
                                    let t = this.current_tab_mut();
                                    t.plot2d_zoom = (t.plot2d_zoom * 1.25).min(8.0);
                                    cx.notify();
                                }),
                            ))
                            .child(cell_pill(
                                ("p2-zout", cell_index),
                                "Zoom -",
                                cx.listener(|this, _, _window, cx| {
                                    let t = this.current_tab_mut();
                                    t.plot2d_zoom = (t.plot2d_zoom / 1.25).max(0.25);
                                    cx.notify();
                                }),
                            ))
                            .child(cell_pill(
                                ("p2-panl", cell_index),
                                "←",
                                cx.listener(|this, _, _window, cx| {
                                    let t = this.current_tab_mut();
                                    t.plot2d_pan_x -= 0.75 / t.plot2d_zoom;
                                    cx.notify();
                                }),
                            ))
                            .child(cell_pill(
                                ("p2-panr", cell_index),
                                "→",
                                cx.listener(|this, _, _window, cx| {
                                    let t = this.current_tab_mut();
                                    t.plot2d_pan_x += 0.75 / t.plot2d_zoom;
                                    cx.notify();
                                }),
                            ))
                            .child(cell_pill(
                                ("p2-rst", cell_index),
                                "Reset",
                                cx.listener(|this, _, _window, cx| {
                                    let t = this.current_tab_mut();
                                    t.plot2d_zoom = 1.0;
                                    t.plot2d_pan_x = 0.0;
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .child(div().flex().flex_row().flex_wrap().gap_3().children(
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
            ))
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
                                paint_plot_canvas(bounds, &data, &palette, window, 2.0, 16.0);
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
                        view_plot.x_label, view_plot.x_min, view_plot.x_max
                    ))
                    .child(format!(
                        "y in [{:.3}, {:.3}]",
                        view_plot.y_min, view_plot.y_max
                    )),
            )
    }

    fn render_plot3d_card(
        &self,
        cell_index: usize,
        surf: &Plot3dData,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tab = self.current_tab();
        let yaw = tab.plot3d_yaw_deg;
        let pitch = tab.plot3d_pitch_deg;
        let surf_data = surf.clone();

        div()
            .w_full()
            .max_w(px(640.))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0x334155))
                            .child(format!(
                                "3D Surface: {} (yaw {:.0}°, pitch {:.0}°)",
                                surf.title, yaw, pitch
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(cell_pill(
                                ("p3-yl", cell_index),
                                "⟲",
                                cx.listener(|this, _, _window, cx| {
                                    this.current_tab_mut().plot3d_yaw_deg -= 15.0;
                                    cx.notify();
                                }),
                            ))
                            .child(cell_pill(
                                ("p3-yr", cell_index),
                                "⟳",
                                cx.listener(|this, _, _window, cx| {
                                    this.current_tab_mut().plot3d_yaw_deg += 15.0;
                                    cx.notify();
                                }),
                            ))
                            .child(cell_pill(
                                ("p3-pu", cell_index),
                                "↑",
                                cx.listener(|this, _, _window, cx| {
                                    let t = this.current_tab_mut();
                                    t.plot3d_pitch_deg = (t.plot3d_pitch_deg + 10.0).min(80.0);
                                    cx.notify();
                                }),
                            ))
                            .child(cell_pill(
                                ("p3-pd", cell_index),
                                "↓",
                                cx.listener(|this, _, _window, cx| {
                                    let t = this.current_tab_mut();
                                    t.plot3d_pitch_deg = (t.plot3d_pitch_deg - 10.0).max(-20.0);
                                    cx.notify();
                                }),
                            ))
                            .child(cell_pill(
                                ("p3-rst", cell_index),
                                "Reset View",
                                cx.listener(|this, _, _window, cx| {
                                    let t = this.current_tab_mut();
                                    t.plot3d_yaw_deg = 42.0;
                                    t.plot3d_pitch_deg = 26.0;
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .h(px(260.))
                    .rounded_md()
                    .bg(rgb(0xf8fafc))
                    .border_1()
                    .border_color(rgb(0xcbd5e1))
                    .child(
                        canvas(
                            move |_bounds, _window, _cx| (surf_data.clone(), yaw, pitch),
                            move |bounds, (data, yaw_deg, pitch_deg), window, _cx| {
                                paint_plot3d_canvas(bounds, &data, yaw_deg, pitch_deg, window);
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
                        "{} in [{:.2}, {:.2}], {} in [{:.2}, {:.2}]",
                        surf.x_label, surf.x_min, surf.x_max, surf.y_label, surf.y_min, surf.y_max
                    ))
                    .child(format!("z in [{:.3}, {:.3}]", surf.z_min, surf.z_max)),
            )
    }

    fn render_deplot_tutor(
        &self,
        preset_idx: usize,
        x0: f64,
        y0: f64,
        show_arrows: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let presets = de_presets();
        let scene = simulate_de_scene(preset_idx, x0, y0);
        let scene_paint = scene.clone();

        div()
            .id("deplot-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .p_5()
                    .pb_12()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(900.))
                            .rounded_md()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xd4dde7))
                            .p_6()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0x0f172a))
                                            .child("DEtools[DEplot][interactive] — Differential Equation Phase Tutor"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x475569))
                                            .child(format!("{} | {}", scene.preset.eq1, scene.preset.eq2)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
                                    .gap_1p5()
                                    .children(presets.iter().enumerate().map(|(idx, p)| {
                                        let active = idx == preset_idx;
                                        let ic = p.default_ic;
                                        div()
                                            .id(("de-preset", idx))
                                            .px_2p5()
                                            .py_1()
                                            .rounded_sm()
                                            .bg(if active { rgb(0x1d4ed8) } else { rgb(0xf1f5f9) })
                                            .border_1()
                                            .border_color(if active { rgb(0x1e40af) } else { rgb(0xcbd5e1) })
                                            .cursor_pointer()
                                            .tooltip(make_tooltip(format!("{} ({})", p.name, p.eq2)))
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if active { rgb(0xffffff) } else { rgb(0x1e293b) })
                                            .on_click(cx.listener(move |this, _, _window, cx| {
                                                if let DocumentContent::DePlotTutor {
                                                    preset_idx: ref mut pi,
                                                    x0: ref mut cx0,
                                                    y0: ref mut cy0,
                                                    ..
                                                } = this.current_tab_mut().content
                                                {
                                                    *pi = idx;
                                                    *cx0 = ic.0;
                                                    *cy0 = ic.1;
                                                }
                                                this.status_message = format!("Selected DE system: {}", p.name);
                                                cx.notify();
                                            }))
                                            .child(p.name)
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_2()
                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x334155)).child(format!("Initial condition: x(0) = {:.2}, v(0) = {:.2}", x0, y0)))
                                    .child(cell_pill(("de-x0m", 0), "x0 -", cx.listener(|this, _, _, cx| {
                                        if let DocumentContent::DePlotTutor { x0: ref mut x, .. } = this.current_tab_mut().content { *x -= 0.3; }
                                        cx.notify();
                                    })))
                                    .child(cell_pill(("de-x0p", 0), "x0 +", cx.listener(|this, _, _, cx| {
                                        if let DocumentContent::DePlotTutor { x0: ref mut x, .. } = this.current_tab_mut().content { *x += 0.3; }
                                        cx.notify();
                                    })))
                                    .child(cell_pill(("de-y0m", 0), "v0 -", cx.listener(|this, _, _, cx| {
                                        if let DocumentContent::DePlotTutor { y0: ref mut y, .. } = this.current_tab_mut().content { *y -= 0.3; }
                                        cx.notify();
                                    })))
                            .child(cell_pill(("de-y0p", 0), "v0 +", cx.listener(|this, _, _, cx| {
                                if let DocumentContent::DePlotTutor { y0: ref mut y, .. } = this.current_tab_mut().content { *y += 0.3; }
                                cx.notify();
                            })))
                                    .child(cell_pill(("de-arr", 0), if show_arrows { "Hide Field" } else { "Show Field" }, cx.listener(|this, _, _, cx| {
                                        if let DocumentContent::DePlotTutor { show_arrows: ref mut s, .. } = this.current_tab_mut().content { *s = !*s; }
                                        cx.notify();
                                    })))
                                    .child(cell_pill(("de-solve", 0), "Run dsolve in Worksheet", cx.listener(|this, _, window, cx| {
                                        let cmd = "dsolve(diff(y, x$2) + y = 0, y);";
                                        this.open_new_worksheet_tab(None);
                                        this.run_statement_in_active_tab(cmd, true, cx);
                                        window.focus(&this.focus_handle);
                                    }))),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .h(px(360.))
                                    .rounded_md()
                                    .bg(rgb(0xf8fafc))
                                    .border_1()
                                    .border_color(rgb(0xcbd5e1))
                                    .child(
                                        canvas(
                                            move |_bounds, _window, _cx| (scene_paint.clone(), show_arrows),
                                            move |bounds, (sc, arrows), window, _cx| {
                                                paint_de_scene_canvas(bounds, &sc, arrows, window);
                                            },
                                        )
                                        .size_full(),
                                    ),
                            ),
                    ),
            )
    }

    fn render_study_guide(
        &self,
        chapter_idx: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let chapters = study_guide_chapters();
        let chapter = chapters.get(chapter_idx).unwrap_or(&chapters[0]);

        div()
            .id("study-guide-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .p_5()
                    .pb_12()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(880.))
                            .rounded_md()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xd4dde7))
                            .p_7()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(div().flex().flex_row().flex_wrap().gap_1p5().children(
                                chapters.iter().enumerate().map(|(i, c)| {
                                    let active = i == chapter_idx;
                                    div()
                                        .id(("sg-chap", i))
                                        .px_2p5()
                                        .py_1()
                                        .rounded_sm()
                                        .bg(if active { rgb(0x1d4ed8) } else { rgb(0xf1f5f9) })
                                        .border_1()
                                        .border_color(if active {
                                            rgb(0x1e40af)
                                        } else {
                                            rgb(0xcbd5e1)
                                        })
                                        .cursor_pointer()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if active {
                                            rgb(0xffffff)
                                        } else {
                                            rgb(0x1e293b)
                                        })
                                        .on_click(cx.listener(move |this, _, _window, cx| {
                                            if let DocumentContent::StudyGuide {
                                                chapter_idx: ref mut ci,
                                            } = this.current_tab_mut().content
                                            {
                                                *ci = i;
                                            }
                                            cx.notify();
                                        }))
                                        .child(c.guide)
                                }),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0x0f172a))
                                            .child(chapter.guide),
                                    )
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0x1d4ed8))
                                            .child(chapter.title),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x475569))
                                            .child(chapter.intro),
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0x0f172a))
                                    .child("▾ Typical Problems"),
                            )
                            .children(chapter.problems.iter().enumerate().map(|(pi, prob)| {
                                let cmd = prob.maple_cmd;
                                div()
                                    .p_2p5()
                                    .rounded_md()
                                    .bg(rgb(0xf8fafc))
                                    .border_1()
                                    .border_color(rgb(0xe2e8f0))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(0x0f172a))
                                                    .child(format!(
                                                        "{}.   {}",
                                                        prob.number, prob.prompt_text
                                                    )),
                                            )
                                            .child(
                                                div().text_xs().text_color(rgb(0x64748b)).child(
                                                    format!(
                                                        "Hint: {}   Command: {}",
                                                        prob.hint, prob.maple_cmd
                                                    ),
                                                ),
                                            ),
                                    )
                                    .child(cell_pill(
                                        ("sg-run", pi),
                                        "Solve in Worksheet",
                                        cx.listener(move |this, _, window, cx| {
                                            this.open_new_worksheet_tab(None);
                                            this.run_statement_in_active_tab(cmd, true, cx);
                                            window.focus(&this.focus_handle);
                                        }),
                                    ))
                            })),
                    ),
            )
    }

    fn render_help_browser(
        &self,
        article_idx: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let articles = help_articles();
        let active = articles.get(article_idx).unwrap_or(&articles[0]);
        let example = active.example_cmd;

        div()
            .id("help-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .p_5()
                    .pb_12()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(920.))
                            .rounded_md()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xd4dde7))
                            .p_5()
                            .flex()
                            .flex_row()
                            .gap_4()
                            .child(
                                div()
                                    .w(px(260.))
                                    .border_r_1()
                                    .border_color(rgb(0xe2e8f0))
                                    .pr_3()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .pb_1()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0x334155))
                                            .child("Table of Contents"),
                                    )
                                    .children(articles.iter().enumerate().map(|(i, art)| {
                                        let is_cur = i == article_idx;
                                        div()
                                            .id(("help-toc", i))
                                            .px_2()
                                            .py_1()
                                            .rounded_sm()
                                            .bg(if is_cur { rgb(0xdbeafe) } else { rgb(0xffffff) })
                                            .hover(|s| s.bg(rgb(0xf1f5f9)))
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _window, cx| {
                                                if let DocumentContent::HelpBrowser {
                                                    article_idx: ref mut ai,
                                                } = this.current_tab_mut().content
                                                {
                                                    *ai = i;
                                                }
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(0x0f172a))
                                                    .child(art.title),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(0x64748b))
                                                    .child(art.category),
                                            )
                                    })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(0x1d4ed8))
                                            .child(active.category),
                                    )
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0x0f172a))
                                            .child(active.title),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x334155))
                                            .child(active.summary),
                                    )
                                    .child(
                                        div()
                                            .p_3()
                                            .rounded_md()
                                            .bg(rgb(0xf8fafc))
                                            .border_1()
                                            .border_color(rgb(0xcbd5e1))
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(0x991b1b))
                                                    .child(format!("[> {}", active.example_cmd)),
                                            )
                                            .child(cell_pill(
                                                ("hlp-try", article_idx),
                                                "Run Example",
                                                cx.listener(move |this, _, window, cx| {
                                                    this.open_new_worksheet_tab(None);
                                                    this.run_statement_in_active_tab(
                                                        example, true, cx,
                                                    );
                                                    window.focus(&this.focus_handle);
                                                }),
                                            )),
                                    ),
                            ),
                    ),
            )
    }
}

fn math_span(text: impl Into<gpui::SharedString>, zoom: f32) -> gpui::AnyElement {
    div()
        .text_size(px(16.0 * zoom))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(0x1d4ed8))
        .child(text.into())
        .into_any_element()
}

fn map_math_symbol(sym: &str) -> &str {
    match sym {
        "Pi" | "pi" => "π",
        "infinity" => "∞",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "theta" => "θ",
        "lambda" => "λ",
        "sigma" => "σ",
        "omega" => "ω",
        _ => sym,
    }
}

fn wrap_parens(inner: gpui::AnyElement, zoom: f32) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .child(math_span("(", zoom))
        .child(inner)
        .child(math_span(")", zoom))
        .into_any_element()
}

fn render_stacked_fraction(num: gpui::AnyElement, den: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .px_1()
        .flex()
        .flex_col()
        .items_center()
        .gap_0p5()
        .child(num)
        .child(div().w_full().min_w(px(14.)).h(px(1.5)).bg(rgb(0x1d4ed8)))
        .child(den)
        .into_any_element()
}

fn render_2d_math_node(expr: &Expr, parent_prec: u8, is_rhs: bool, zoom: f32) -> gpui::AnyElement {
    match expr {
        Expr::Rational(r) => {
            if r.is_integer() {
                let s = r.num.to_string();
                let node = math_span(s, zoom);
                if r.is_negative() && parent_prec >= 5 && !is_rhs {
                    wrap_parens(node, zoom)
                } else {
                    node
                }
            } else {
                let num_node = math_span(r.num.abs().to_string(), zoom * 0.88);
                let den_node = math_span(r.den.to_string(), zoom * 0.88);
                let frac = render_stacked_fraction(num_node, den_node);
                let signed = if r.is_negative() {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_0p5()
                        .child(math_span("−", zoom))
                        .child(frac)
                        .into_any_element()
                } else {
                    frac
                };
                if parent_prec >= 5 {
                    wrap_parens(signed, zoom)
                } else {
                    signed
                }
            }
        }
        Expr::Float(v) => {
            let clean = if *v == 0.0 { 0.0 } else { *v };
            let node = math_span(format!("{clean}"), zoom);
            if *v < 0.0 && parent_prec >= 5 && !is_rhs {
                wrap_parens(node, zoom)
            } else {
                node
            }
        }
        Expr::Symbol(s) => math_span(map_math_symbol(s).to_string(), zoom),
        Expr::UnaryNeg(inner) => {
            let row = div()
                .flex()
                .flex_row()
                .items_center()
                .child(math_span("−", zoom))
                .child(render_2d_math_node(inner, 4, false, zoom))
                .into_any_element();
            if parent_prec > 4 || (parent_prec == 5 && !is_rhs) {
                wrap_parens(row, zoom)
            } else {
                row
            }
        }
        Expr::Add(_, _) | Expr::Sub(_, _) => {
            let mut terms: Vec<(bool, &Expr)> = Vec::new();
            collect_2d_terms(expr, false, &mut terms);
            let mut row = div().flex().flex_row().flex_wrap().items_center().gap_1();
            for (i, (neg, term)) in terms.into_iter().enumerate() {
                if i == 0 {
                    if neg {
                        row = row.child(math_span("−", zoom));
                    }
                    row = row.child(render_2d_math_node(term, 2, false, zoom));
                } else if !neg {
                    if let Some(pos) = extract_2d_neg(term) {
                        row = row
                            .child(math_span("−", zoom))
                            .child(render_2d_math_node(&pos, 2, true, zoom));
                    } else {
                        row = row
                            .child(math_span("+", zoom))
                            .child(render_2d_math_node(term, 2, false, zoom));
                    }
                } else if let Some(pos) = extract_2d_neg(term) {
                    row = row
                        .child(math_span("+", zoom))
                        .child(render_2d_math_node(&pos, 2, false, zoom));
                } else {
                    row = row
                        .child(math_span("−", zoom))
                        .child(render_2d_math_node(term, 2, true, zoom));
                }
            }
            let elem = row.into_any_element();
            if parent_prec > 2 || (parent_prec == 2 && is_rhs) {
                wrap_parens(elem, zoom)
            } else {
                elem
            }
        }
        Expr::Mul(lhs, rhs) => {
            let elem = if let Expr::Rational(r) = lhs.as_ref()
                && r.is_neg_one()
            {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(math_span("−", zoom))
                    .child(render_2d_math_node(rhs, 4, false, zoom))
                    .into_any_element()
            } else {
                let mut row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(render_2d_math_node(lhs, 3, false, zoom));
                if matches!(rhs.as_ref(), Expr::Rational(_) | Expr::Float(_)) {
                    row = row.child(math_span("·", zoom));
                }
                row.child(render_2d_math_node(rhs, 3, true, zoom))
                    .into_any_element()
            };
            if parent_prec > 3 {
                wrap_parens(elem, zoom)
            } else {
                elem
            }
        }
        Expr::Div(lhs, rhs) => {
            let num_node = render_2d_math_node(lhs, 0, false, zoom * 0.92);
            let den_node = render_2d_math_node(rhs, 0, false, zoom * 0.92);
            let frac = render_stacked_fraction(num_node, den_node);
            if parent_prec > 3 {
                wrap_parens(frac, zoom)
            } else {
                frac
            }
        }
        Expr::Pow(base, exp) => {
            let base_node = render_2d_math_node(base, 5, false, zoom);
            let exp_node =
                div()
                    .mb(px(8.0 * zoom))
                    .child(render_2d_math_node(exp, 0, true, zoom * 0.72));
            let pow = div()
                .flex()
                .flex_row()
                .items_start()
                .child(base_node)
                .child(exp_node)
                .into_any_element();
            if parent_prec > 5 || (parent_prec == 5 && !is_rhs) {
                wrap_parens(pow, zoom)
            } else {
                pow
            }
        }
        Expr::Call(name, args) => match name.as_str() {
            "sqrt" if args.len() == 1 => div()
                .flex()
                .flex_row()
                .items_center()
                .child(math_span("√", zoom))
                .child(
                    div()
                        .px_1()
                        .border_t_1()
                        .border_color(rgb(0x1d4ed8))
                        .child(render_2d_math_node(&args[0], 0, false, zoom)),
                )
                .into_any_element(),
            "__arrow__" if args.len() == 2 => div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(match &args[0] {
                    Expr::List(params) if params.len() == 1 => {
                        render_2d_math_node(&params[0], 0, false, zoom)
                    }
                    other => render_2d_math_node(other, 0, false, zoom),
                })
                .child(math_span("→", zoom))
                .child(render_2d_math_node(&args[1], 0, false, zoom))
                .into_any_element(),
            _ => {
                let mut row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(math_span(format!("{name}("), zoom));
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        row = row.child(math_span(", ", zoom));
                    }
                    row = row.child(render_2d_math_node(a, 0, false, zoom));
                }
                row.child(math_span(")", zoom)).into_any_element()
            }
        },
        Expr::Equation(lhs, rhs) => div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(render_2d_math_node(lhs, 1, false, zoom))
            .child(math_span("=", zoom))
            .child(render_2d_math_node(rhs, 1, true, zoom))
            .into_any_element(),
        Expr::Inequality(op, lhs, rhs) => div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(render_2d_math_node(lhs, 1, false, zoom))
            .child(math_span(op.pretty().trim().to_string(), zoom))
            .child(render_2d_math_node(rhs, 1, true, zoom))
            .into_any_element(),
        Expr::Range(lhs, rhs) => div()
            .flex()
            .flex_row()
            .items_center()
            .child(render_2d_math_node(lhs, 1, false, zoom))
            .child(math_span("..", zoom))
            .child(render_2d_math_node(rhs, 1, true, zoom))
            .into_any_element(),
        Expr::List(items) => {
            let mut row = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .child(math_span("[", zoom));
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    row = row.child(math_span(", ", zoom));
                }
                row = row.child(render_2d_math_node(item, 0, false, zoom));
            }
            row.child(math_span("]", zoom)).into_any_element()
        }
        Expr::Matrix(rows) => div()
            .px_3()
            .py_1p5()
            .border_l_2()
            .border_r_2()
            .border_color(rgb(0x1d4ed8))
            .rounded_sm()
            .flex()
            .flex_col()
            .gap_1p5()
            .children(rows.iter().map(|row| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .children(row.iter().map(|cell| {
                        div()
                            .min_w(px(40.0 * zoom))
                            .flex()
                            .justify_center()
                            .items_center()
                            .child(render_2d_math_node(cell, 0, false, zoom * 0.94))
                    }))
            }))
            .into_any_element(),
    }
}

fn collect_2d_terms<'a>(expr: &'a Expr, negate: bool, out: &mut Vec<(bool, &'a Expr)>) {
    match expr {
        Expr::Add(a, b) => {
            collect_2d_terms(a, negate, out);
            collect_2d_terms(b, negate, out);
        }
        Expr::Sub(a, b) => {
            collect_2d_terms(a, negate, out);
            collect_2d_terms(b, !negate, out);
        }
        other => out.push((negate, other)),
    }
}

fn extract_2d_neg(expr: &Expr) -> Option<Expr> {
    match expr {
        Expr::Rational(r) if r.is_negative() => Some(Expr::Rational(-*r)),
        Expr::Float(v) if *v < 0.0 => Some(Expr::Float(-*v)),
        Expr::UnaryNeg(inner) => Some((**inner).clone()),
        Expr::Mul(lhs, rhs) => {
            if let Expr::Rational(r) = lhs.as_ref()
                && r.is_negative()
            {
                let pos = -*r;
                if pos.is_one() {
                    return Some((**rhs).clone());
                }
                return Some(Expr::Mul(Box::new(Expr::Rational(pos)), rhs.clone()));
            }
            if let Expr::Float(v) = lhs.as_ref()
                && *v < 0.0
            {
                return Some(Expr::Mul(Box::new(Expr::Float(-*v)), rhs.clone()));
            }
            None
        }
        Expr::Div(lhs, rhs) => extract_2d_neg(lhs).map(|p| Expr::Div(Box::new(p), rhs.clone())),
        _ => None,
    }
}

fn paint_plot_canvas(
    bounds: Bounds<Pixels>,
    data: &PlotData,
    palette: &[Rgba],
    window: &mut Window,
    stroke_width: f32,
    pad: f32,
) {
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
            Bounds::new(point(ax, px(top)), size(px(1.2), px(height))),
            rgb(0x94a3b8),
        ));
    }
    if data.y_min <= 0.0 && 0.0 <= data.y_max {
        let ay = map_y(0.0);
        window.paint_quad(fill(
            Bounds::new(point(px(left), ay), size(px(width), px(1.2))),
            rgb(0x94a3b8),
        ));
    }

    for (idx, series) in data.series.iter().enumerate() {
        let color: Hsla = palette[idx % palette.len()].into();
        let mut builder = PathBuilder::stroke(px(stroke_width));
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

fn paint_plot3d_canvas(
    bounds: Bounds<Pixels>,
    surf: &Plot3dData,
    yaw_deg: f64,
    pitch_deg: f64,
    window: &mut Window,
) {
    let w = f32::from(bounds.size.width).max(1.0);
    let h = f32::from(bounds.size.height).max(1.0);
    let cx = f32::from(bounds.left()) + 0.5 * w;
    let cy = f32::from(bounds.top()) + 0.52 * h;
    let scale = w.min(h) * 0.34;

    let yaw = yaw_deg.to_radians();
    let pitch = pitch_deg.to_radians();
    let (sy, cy_a) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();

    let x_mid = 0.5 * (surf.x_min + surf.x_max);
    let y_mid = 0.5 * (surf.y_min + surf.y_max);
    let z_mid = 0.5 * (surf.z_min + surf.z_max);
    let x_half = (0.5 * (surf.x_max - surf.x_min)).max(1e-9);
    let y_half = (0.5 * (surf.y_max - surf.y_min)).max(1e-9);
    let z_half = (0.5 * (surf.z_max - surf.z_min)).max(1e-9);

    let project = |x: f64, y: f64, z: f64| -> Option<(f32, f32, f64)> {
        if !z.is_finite() {
            return None;
        }
        let nx = (x - x_mid) / x_half;
        let ny = (y - y_mid) / y_half;
        let nz = (z - z_mid) / z_half;
        let rx = nx * cy_a - ny * sy;
        let ry = nx * sy + ny * cy_a;
        let sx = cx + (rx as f32) * scale;
        let sy_scr = cy - ((nz * cp + ry * sp) as f32) * scale * 0.85;
        let zn = ((z - surf.z_min) / (2.0 * z_half)).clamp(0.0, 1.0);
        Some((sx, sy_scr, zn))
    };

    // Paint colored mesh rows and columns.
    for row in &surf.grid {
        for pair in row.windows(2) {
            if let (Some((x1, y1, z1)), Some((x2, y2, z2))) = (
                project(pair[0].0, pair[0].1, pair[0].2),
                project(pair[1].0, pair[1].1, pair[1].2),
            ) {
                let z_avg = 0.5 * (z1 + z2);
                let color: Hsla = height_color(z_avg).into();
                let mut pb = PathBuilder::stroke(px(1.5));
                pb.move_to(point(px(x1), px(y1)));
                pb.line_to(point(px(x2), px(y2)));
                if let Ok(path) = pb.build() {
                    window.paint_path(path, color);
                }
            }
        }
    }
    for c in 0..surf.cols {
        for r in 0..surf.rows.saturating_sub(1) {
            let p1 = surf.grid[r][c];
            let p2 = surf.grid[r + 1][c];
            if let (Some((x1, y1, z1)), Some((x2, y2, z2))) =
                (project(p1.0, p1.1, p1.2), project(p2.0, p2.1, p2.2))
            {
                let z_avg = 0.5 * (z1 + z2);
                let color: Hsla = height_color(z_avg).into();
                let mut pb = PathBuilder::stroke(px(1.2));
                pb.move_to(point(px(x1), px(y1)));
                pb.line_to(point(px(x2), px(y2)));
                if let Ok(path) = pb.build() {
                    window.paint_path(path, color);
                }
            }
        }
    }
}

fn height_color(t: f64) -> Rgba {
    if t < 0.33 {
        rgb(0x1d4ed8)
    } else if t < 0.66 {
        rgb(0x059669)
    } else {
        rgb(0xd97706)
    }
}

fn paint_de_scene_canvas(
    bounds: Bounds<Pixels>,
    scene: &crate::tutor::DePlotScene,
    show_arrows: bool,
    window: &mut Window,
) {
    let pad = 20.0f32;
    let left = f32::from(bounds.left()) + pad;
    let right = f32::from(bounds.right()) - pad;
    let top = f32::from(bounds.top()) + pad;
    let bottom = f32::from(bounds.bottom()) - pad;
    let w = (right - left).max(1.0);
    let h = (bottom - top).max(1.0);
    let (xmin, xmax) = scene.preset.x_range;
    let (ymin, ymax) = scene.preset.y_range;
    let xs = (xmax - xmin).max(1e-9);
    let ys = (ymax - ymin).max(1e-9);

    let mx = |x: f64| px(left + (((x - xmin) / xs) as f32) * w);
    let my = |y: f64| px(bottom - (((y - ymin) / ys) as f32) * h);

    if xmin <= 0.0 && 0.0 <= xmax {
        window.paint_quad(fill(
            Bounds::new(point(mx(0.0), px(top)), size(px(1.2), px(h))),
            rgb(0x94a3b8),
        ));
    }
    if ymin <= 0.0 && 0.0 <= ymax {
        window.paint_quad(fill(
            Bounds::new(point(px(left), my(0.0)), size(px(w), px(1.2))),
            rgb(0x94a3b8),
        ));
    }

    if show_arrows {
        let arrow_col: Hsla = rgb(0x64748b).into();
        let step_len = xs / 28.0;
        let step_y = ys / 24.0;
        for &(x, y, dx, dy) in &scene.field_arrows {
            let mut pb = PathBuilder::stroke(px(1.1));
            pb.move_to(point(
                mx(x - 0.5 * step_len * dx),
                my(y - 0.5 * step_y * dy),
            ));
            pb.line_to(point(
                mx(x + 0.5 * step_len * dx),
                my(y + 0.5 * step_y * dy),
            ));
            if let Ok(path) = pb.build() {
                window.paint_path(path, arrow_col);
            }
        }
    }

    if scene.trajectory.len() >= 2 {
        let traj_col: Hsla = rgb(0x2563eb).into();
        let mut pb = PathBuilder::stroke(px(2.4));
        for (i, &(x, y)) in scene.trajectory.iter().enumerate() {
            let p = point(mx(x), my(y));
            if i == 0 {
                pb.move_to(p);
            } else {
                pb.line_to(p);
            }
        }
        if let Ok(path) = pb.build() {
            window.paint_path(path, traj_col);
        }
        window.paint_quad(fill(
            Bounds::new(
                point(mx(scene.x0) - px(4.), my(scene.y0) - px(4.)),
                size(px(8.), px(8.)),
            ),
            rgb(0xdc2626),
        ));
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

        let bounds = Bounds::centered(None, size(px(1300.0), px(860.0)), cx);
        let window = cx
            .open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("Maple Worksheet — [Local Kernel]".into()),
                        ..Default::default()
                    }),
                    window_decorations: Some(WindowDecorations::Client),
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
