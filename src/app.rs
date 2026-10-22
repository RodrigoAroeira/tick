use std::{
    io::{self, Write},
    mem,
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::event::KeyCode;
use unicode_width::UnicodeWidthStr;

use crate::action::{Action, InsertAction, TabAction};
use crate::helpers::{
    clear_scr, get_key_event, get_todos_dones, goto, goto_begin, handle_term_size, init_scr,
    save_to_file, split_to_fit, write_text,
};
use crate::{globals, screen_guard::ScreenGuard, tab::Tab};

#[derive(Clone)]
pub(crate) enum InsertMode {
    New,
    Edit(String),
}

#[derive(Clone)]
pub(crate) enum Mode {
    Normal,
    Insert(InsertMode),
    Help,
}

#[derive(Clone)]
pub struct AppState {
    pub todos: Vec<String>,
    pub dones: Vec<String>,
    pub todos_idx: usize,
    pub dones_idx: usize,
    pub curr_tab: Tab,
    pub show_number: bool,
    pub mode: Mode,
}

pub(crate) enum AppSignal {
    SaveAndQuit,
    NoSave,
    TerminalError(io::Error),
}

impl From<io::Error> for AppSignal {
    fn from(e: io::Error) -> Self {
        AppSignal::TerminalError(e)
    }
}

impl AppState {
    fn enter_press(&mut self) {
        match self.curr_tab {
            Tab::Todos => {
                if self.todos.is_empty() {
                    return;
                }
                let value = self.todos.remove(self.todos_idx);
                self.dones.push(value);
            }
            Tab::Dones => {
                if self.dones.is_empty() {
                    return;
                }
                let value = self.dones.remove(self.dones_idx);
                self.todos.push(value);
            }
        }
    }

    fn switch_tab(&mut self, tab_action: TabAction) {
        match tab_action {
            TabAction::Toggle => self.curr_tab = self.curr_tab.toggle(),
            TabAction::Left => self.curr_tab = Tab::Todos,
            TabAction::Right => self.curr_tab = Tab::Dones,
        }
    }

    fn cursor_move(&mut self, direction: KeyCode) {
        let idx = match self.curr_tab {
            Tab::Todos => &mut self.todos_idx,
            Tab::Dones => &mut self.dones_idx,
        };
        match direction {
            KeyCode::Down => *idx += 1,
            KeyCode::Up => *idx = idx.saturating_sub(1),
            _ => unreachable!("This spot should't be reachable"),
        };
    }

    fn delete(&mut self) {
        let (target_vec, idx) = match self.curr_tab {
            Tab::Todos => (&mut self.todos, &self.todos_idx),
            Tab::Dones => (&mut self.dones, &self.dones_idx),
        };
        if target_vec.is_empty() {
            return;
        }
        target_vec.remove(*idx);
    }

    fn move_item(&mut self, direction: KeyCode) {
        let (vec, idx) = match self.curr_tab {
            Tab::Todos => (&mut self.todos, &mut self.todos_idx),
            Tab::Dones => (&mut self.dones, &mut self.dones_idx),
        };
        if vec.is_empty() {
            return;
        }
        let idx_val = *idx;
        let new_idx = match direction {
            KeyCode::Down => (idx_val + 1).min(vec.len() - 1),
            KeyCode::Up => idx_val.saturating_sub(1),
            _ => unreachable!(),
        };
        vec.swap(idx_val, new_idx);
        *idx = new_idx;
    }

    fn start_insert(&mut self, direction: KeyCode) {
        self.mode = Mode::Insert(InsertMode::New);
        let (list, idx) = match self.curr_tab {
            Tab::Todos => (&mut self.todos, &mut self.todos_idx),
            Tab::Dones => (&mut self.dones, &mut self.dones_idx),
        };
        let insert_idx = match direction {
            KeyCode::Up => *idx,
            KeyCode::Down => *idx + 1,
            _ => unreachable!(),
        }
        .clamp(0, list.len());
        list.insert(insert_idx, String::new());
        *idx = insert_idx;
    }

    fn start_edit(&mut self) {
        if let Some(snap) = self.get_current_buffer().cloned() {
            self.mode = Mode::Insert(InsertMode::Edit(snap));
        }
    }

    fn insert_action(&mut self, action: InsertAction) {
        let buf = match self.curr_tab {
            Tab::Todos => self.todos.get_mut(self.todos_idx).unwrap(),
            Tab::Dones => self.dones.get_mut(self.dones_idx).unwrap(),
        };
        match action {
            InsertAction::Enter => self.mode = Mode::Normal,
            InsertAction::Cancel => {
                match mem::replace(&mut self.mode, Mode::Normal) {
                    Mode::Insert(InsertMode::Edit(snap)) => *buf = snap,
                    Mode::Insert(InsertMode::New) => self.delete(),
                    _ => unreachable!(),
                };
            }
            InsertAction::Char(c) => buf.push(c),
            InsertAction::DeleteChar => _ = buf.pop(),
        }
    }

    fn handle_help_action(&mut self, action: &Action) {
        match action {
            Action::SaveQuit | Action::NoSaveQuit => self.mode = Mode::Normal,
            _ => {}
        }
    }

    fn show_help(&mut self) {
        self.mode = Mode::Help;
    }

    fn toggle_show_number(&mut self) {
        self.show_number = !self.show_number;
    }

    fn get_current_buffer(&self) -> Option<&String> {
        match self.curr_tab {
            Tab::Todos => self.todos.get(self.todos_idx),
            Tab::Dones => self.dones.get(self.dones_idx),
        }
    }

    fn goto_list_pos(&mut self, pos: usize) {
        let idx = match self.curr_tab {
            Tab::Todos => &mut self.todos_idx,
            Tab::Dones => &mut self.dones_idx,
        };
        *idx = pos;
    }

    fn clamp_indexes(&mut self) {
        self.todos_idx = self.todos_idx.clamp(0, self.todos.len().saturating_sub(1));
        self.dones_idx = self.dones_idx.clamp(0, self.dones.len().saturating_sub(1));
    }
}

pub struct App {
    file_path: PathBuf,
    curr_state: AppState,
    undo_stack: Vec<AppState>,
    redo_stack: Vec<AppState>,
}

/// Constructor / Entry Point
impl App {
    pub fn new<P>(file_path: P) -> anyhow::Result<Self>
    where
        P: AsRef<Path>,
    {
        let (todos, dones) = get_todos_dones(&file_path)?;
        let s = Self {
            file_path: file_path.as_ref().to_path_buf(),
            curr_state: AppState {
                todos,
                dones,
                todos_idx: 0,
                dones_idx: 0,
                curr_tab: Tab::Todos,
                show_number: false,
                mode: Mode::Normal,
            },
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        };
        Ok(s)
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        match self.main_loop() {
            Err(AppSignal::NoSave) => return Ok(()),
            Err(AppSignal::SaveAndQuit) | Ok(()) => {}
            Err(AppSignal::TerminalError(e)) => {
                let _ = writeln!(io::stderr(), "Terminal error: {e}");
                return Err(e.into());
            }
        }

        if !self.undo_stack.is_empty() {
            save_to_file(
                &self.file_path,
                &self.curr_state.todos,
                &self.curr_state.dones,
            )?;
        }
        Ok(())
    }
}

/// Main loop / Screen Drawing
impl App {
    fn main_loop(&mut self) -> Result<(), AppSignal> {
        let _guard = ScreenGuard {};
        let mut term_size = Default::default();
        init_scr()?;
        loop {
            self.curr_state.clamp_indexes();
            handle_term_size(&mut term_size)?;
            clear_scr()?;
            goto_begin()?;
            self.write_screen(term_size)?;
            self.write_status(term_size)?;

            let mid_scr = term_size.0 / 2;
            match self.curr_state.curr_tab {
                Tab::Todos => goto(0, self.curr_state.todos_idx as u16 + 1)?,
                Tab::Dones => goto(mid_scr, self.curr_state.dones_idx as u16 + 1)?,
            }

            if let Some(event) = get_key_event(Duration::from_millis(1000 / 60))? {
                match self.curr_state.mode {
                    Mode::Normal => {
                        if let Ok(action) = Action::try_from(event) {
                            self.execute_action(action)?;
                        }
                    }
                    Mode::Insert(_) => {
                        if let Ok(action) = InsertAction::try_from(event) {
                            self.curr_state.insert_action(action);
                        }
                    }
                    Mode::Help => {
                        if let Ok(action) = Action::try_from(event) {
                            self.curr_state.handle_help_action(&action);
                        }
                    }
                }
            }
        }
    }

    fn write_screen(&self, term_size: (u16, u16)) -> io::Result<()> {
        if matches!(self.curr_state.mode, Mode::Help) {
            self.write_help_screen(term_size)?;
        } else {
            self.write_header(term_size)?;
            self.write_todos_dones(term_size)?;
        }
        Ok(())
    }

    fn write_header(&self, term_size: (u16, u16)) -> io::Result<()> {
        let col_mid = term_size.0 / 2;
        let is_tab_todo = matches!(self.curr_state.curr_tab, Tab::Todos);

        write_text("TODO", is_tab_todo)?;
        write_text(&" ".repeat(col_mid as usize - 4), false)?;
        write_text("DONE\r\n", !is_tab_todo)?;
        Ok(())
    }

    fn write_status(&self, term_size: (u16, u16)) -> io::Result<()> {
        let (_cols, rows) = term_size;
        goto(0, rows - 1)?;
        let txt = match self.curr_state.mode {
            Mode::Normal => "NORMAL",
            Mode::Insert(InsertMode::New) => "INSERT",
            Mode::Insert(InsertMode::Edit(_)) => "EDIT",
            Mode::Help => "HELP",
        };

        write_text(txt, true)
    }

    fn write_help_screen(&self, term_size: (u16, u16)) -> io::Result<()> {
        let (cols, _) = term_size;

        let println = |s: &str| -> io::Result<()> {
            write_text(s, false)?;
            write_text("\r\n", false)?;
            Ok(())
        };

        println(&"=".repeat(cols as usize))?;
        println("HELP")?;
        println(&"=".repeat(cols as usize))?;
        println("")?;

        let sections: &[(&str, &[(&str, &str)])] = &[
            (
                "ACTIONS",
                &[
                    ("f1", "Show this screen"),
                    ("i / o", "Insert item above / below"),
                    ("e", "Edit item under cursor"),
                    ("J / K", "Move item under cursor down / up"),
                    ("u", "Undo"),
                    ("Ctrl+r", "Redo"),
                    ("q", "Save and quit"),
                    ("Q", "Quit without saving"),
                ],
            ),
            (
                "MOVEMENT",
                &[
                    ("j / k", "Move cursor down / up"),
                    ("g / G", "Jump to beginning / end"),
                    ("Tab", "Toggle Tab"),
                    ("<- / ->", "Change to todo/done tab"),
                ],
            ),
            (
                "INSERT / EDIT MODE",
                &[
                    ("(type normally)", "Edit text"),
                    ("Enter", "Save changes"),
                    ("Esc", "Cancel"),
                ],
            ),
            ("LEAVING HELP", &[("q / Q", "Quit help screen")]),
        ];

        let max_key_len = sections
            .iter()
            .flat_map(|(_, lines)| lines.iter().map(|(k, _)| k.len()))
            .max()
            .unwrap_or(0);

        for (title, lines) in sections.iter() {
            println(title)?;
            for (key, desc) in lines.iter() {
                let padded = format!("{:width$}", key, width = max_key_len);
                println(&format!("  {}  - {}", padded, desc))?;
            }
            println("")?;
        }

        println(&"=".repeat(cols as usize))?;
        Ok(())
    }

    fn write_todos_dones(&self, term_size: (u16, u16)) -> io::Result<()> {
        let (cols, _) = term_size;
        let col_mid = cols / 2;

        let draw_items = |items: &[String],
                          line_begin: &str,
                          is_active_tab: bool,
                          selected_idx: usize,
                          col_offset: u16|
         -> io::Result<()> {
            let mut current_line = 1;
            for (idx, item) in items.iter().enumerate() {
                let should_highlight = is_active_tab && idx == selected_idx;

                let num_width = items.len().to_string().len();

                let line_label = if self.curr_state.show_number {
                    format!("{:>width$}.", idx + 1, width = num_width)
                } else {
                    String::from(line_begin)
                };

                let full_line = format!("{} {}", line_label, item);
                let offset = line_begin.width() + 1;
                let (first_line, rest_lines) = split_to_fit(
                    &full_line,
                    col_mid as usize - if col_offset > 0 { 0 } else { 1 },
                    offset,
                );

                goto(col_offset, current_line)?;
                write_text(first_line, should_highlight)?;
                current_line += 1;

                let padding = " ".repeat(line_begin.width() + 1);
                for line in rest_lines {
                    goto(col_offset, current_line)?;
                    write_text(&format!("{}{}", padding, line), should_highlight)?;
                    current_line += 1;
                }
            }
            Ok(())
        };

        draw_items(
            &self.curr_state.todos,
            globals::TODO_INDICATOR,
            matches!(self.curr_state.curr_tab, Tab::Todos),
            self.curr_state.todos_idx,
            0,
        )?;
        draw_items(
            &self.curr_state.dones,
            globals::DONE_INDICATOR,
            matches!(self.curr_state.curr_tab, Tab::Dones),
            self.curr_state.dones_idx,
            col_mid,
        )?;
        Ok(())
    }
}

/// Actions / Mode Handling
impl App {
    fn execute_action(&mut self, action: Action) -> Result<(), AppSignal> {
        match action {
            Action::Enter => {
                self.push_state();
                self.curr_state.enter_press();
            }
            Action::SwitchTab(tab) => self.curr_state.switch_tab(tab),
            Action::Insert(direction) => {
                self.push_state();
                self.curr_state.start_insert(direction);
            }
            Action::Edit => {
                self.push_state();
                self.curr_state.start_edit();
            }
            Action::MoveCursor(direction) => self.curr_state.cursor_move(direction),
            Action::MoveItem(direction) => {
                self.push_state();
                self.curr_state.move_item(direction);
            }
            Action::GotoBegin => self.curr_state.goto_list_pos(0),
            Action::GotoEnd => self.curr_state.goto_list_pos(usize::MAX),
            Action::Delete => {
                self.push_state();
                self.curr_state.delete();
            }
            Action::SaveQuit => return Err(AppSignal::SaveAndQuit),
            Action::NoSaveQuit => return Err(AppSignal::NoSave),
            Action::ShowHelp => self.curr_state.show_help(),
            Action::ShowNumber => self.curr_state.toggle_show_number(),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
        }
        Ok(())
    }
}

/// Utilities / Internal Helpers
impl App {
    fn push_state(&mut self) {
        self.redo_stack.clear();
        self.undo_stack.push(self.curr_state.clone());
    }

    fn undo(&mut self) {
        if let Some(state) = self.undo_stack.pop() {
            self.redo_stack
                .push(mem::replace(&mut self.curr_state, state));
        }
    }

    fn redo(&mut self) {
        if let Some(state) = self.redo_stack.pop() {
            self.undo_stack
                .push(mem::replace(&mut self.curr_state, state));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(todos: &[&str], dones: &[&str]) -> AppState {
        AppState {
            todos: todos.iter().map(|s| s.to_string()).collect(),
            dones: dones.iter().map(|s| s.to_string()).collect(),
            todos_idx: 0,
            dones_idx: 0,
            curr_tab: Tab::Todos,
            show_number: false,
            mode: Mode::Normal,
        }
    }

    #[test]
    fn test_enter_press_moves_todo_to_done() {
        let mut s = state(&["task1", "task2"], &[]);
        s.enter_press();
        assert_eq!(s.todos, vec!["task2"]);
        assert_eq!(s.dones, vec!["task1"]);
    }

    #[test]
    fn test_enter_press_moves_done_to_todo() {
        let mut s = state(&[], &["done1", "done2"]);
        s.curr_tab = Tab::Dones;
        s.enter_press();
        assert_eq!(s.dones, vec!["done2"]);
        assert_eq!(s.todos, vec!["done1"]);
    }

    #[test]
    fn test_enter_press_empty_todos() {
        let mut s = state(&[], &["done1"]);
        s.enter_press();
        assert!(s.todos.is_empty());
        assert_eq!(s.dones, vec!["done1"]);
    }

    #[test]
    fn test_enter_press_empty_dones() {
        let mut s = state(&["task1"], &[]);
        s.curr_tab = Tab::Dones;
        s.enter_press();
        assert!(s.dones.is_empty());
        assert_eq!(s.todos, vec!["task1"]);
    }

    #[test]
    fn test_switch_tab_toggle() {
        let mut s = state(&[], &[]);
        assert!(matches!(s.curr_tab, Tab::Todos));
        s.switch_tab(TabAction::Toggle);
        assert!(matches!(s.curr_tab, Tab::Dones));
        s.switch_tab(TabAction::Toggle);
        assert!(matches!(s.curr_tab, Tab::Todos));
    }

    #[test]
    fn test_switch_tab_left_right() {
        let mut s = state(&[], &[]);
        s.switch_tab(TabAction::Right);
        assert!(matches!(s.curr_tab, Tab::Dones));
        s.switch_tab(TabAction::Left);
        assert!(matches!(s.curr_tab, Tab::Todos));
    }

    #[test]
    fn test_cursor_move_down_in_todos() {
        let mut s = state(&["a", "b", "c"], &[]);
        s.cursor_move(KeyCode::Down);
        assert_eq!(s.todos_idx, 1);
        s.cursor_move(KeyCode::Down);
        assert_eq!(s.todos_idx, 2);
    }

    #[test]
    fn test_cursor_move_up_in_todos() {
        let mut s = state(&["a", "b", "c"], &[]);
        s.todos_idx = 2;
        s.cursor_move(KeyCode::Up);
        assert_eq!(s.todos_idx, 1);
    }

    #[test]
    fn test_cursor_move_up_at_zero_stays_zero() {
        let mut s = state(&["a", "b"], &[]);
        s.cursor_move(KeyCode::Up);
        assert_eq!(s.todos_idx, 0);
    }

    #[test]
    fn test_cursor_move_down_in_dones() {
        let mut s = state(&[], &["x", "y"]);
        s.curr_tab = Tab::Dones;
        s.cursor_move(KeyCode::Down);
        assert_eq!(s.dones_idx, 1);
    }

    #[test]
    fn test_delete_removes_from_todos() {
        let mut s = state(&["a", "b", "c"], &[]);
        s.delete();
        assert_eq!(s.todos, vec!["b", "c"]);
    }

    #[test]
    fn test_delete_removes_from_dones() {
        let mut s = state(&[], &["x", "y"]);
        s.curr_tab = Tab::Dones;
        s.delete();
        assert_eq!(s.dones, vec!["y"]);
    }

    #[test]
    fn test_delete_empty_list() {
        let mut s = state(&[], &[]);
        s.delete();
        assert!(s.todos.is_empty());
    }

    #[test]
    fn test_delete_respects_index() {
        let mut s = state(&["a", "b", "c"], &[]);
        s.todos_idx = 1;
        s.delete();
        assert_eq!(s.todos, vec!["a", "c"]);
    }

    #[test]
    fn test_move_item_down_swaps_in_todos() {
        let mut s = state(&["a", "b"], &[]);
        s.move_item(KeyCode::Down);
        assert_eq!(s.todos, vec!["b", "a"]);
        assert_eq!(s.todos_idx, 1);
    }

    #[test]
    fn test_move_item_up_swaps_in_todos() {
        let mut s = state(&["a", "b"], &[]);
        s.todos_idx = 1;
        s.move_item(KeyCode::Up);
        assert_eq!(s.todos, vec!["b", "a"]);
        assert_eq!(s.todos_idx, 0);
    }

    #[test]
    fn test_move_item_down_at_bottom_stays() {
        let mut s = state(&["a", "b"], &[]);
        s.todos_idx = 1;
        s.move_item(KeyCode::Down);
        assert_eq!(s.todos, vec!["a", "b"]);
        assert_eq!(s.todos_idx, 1);
    }

    #[test]
    fn test_move_item_up_at_top_stays() {
        let mut s = state(&["a", "b"], &[]);
        s.move_item(KeyCode::Up);
        assert_eq!(s.todos, vec!["a", "b"]);
        assert_eq!(s.todos_idx, 0);
    }

    #[test]
    fn test_move_item_empty_list() {
        let mut s = state(&[], &[]);
        s.move_item(KeyCode::Down);
        assert!(s.todos.is_empty());
    }

    #[test]
    fn test_start_insert_above_current() {
        let mut s = state(&["b", "c"], &[]);
        s.start_insert(KeyCode::Up);
        assert_eq!(s.todos, vec!["", "b", "c"]);
        assert_eq!(s.todos_idx, 0);
        assert!(matches!(s.mode, Mode::Insert(InsertMode::New)));
    }

    #[test]
    fn test_start_insert_below_current() {
        let mut s = state(&["a", "c"], &[]);
        s.start_insert(KeyCode::Down);
        assert_eq!(s.todos, vec!["a", "", "c"]);
        assert_eq!(s.todos_idx, 1);
        assert!(matches!(s.mode, Mode::Insert(InsertMode::New)));
    }

    #[test]
    fn test_start_insert_at_end() {
        let mut s = state(&["a"], &[]);
        s.todos_idx = 0;
        s.start_insert(KeyCode::Down);
        assert_eq!(s.todos, vec!["a", ""]);
        assert_eq!(s.todos_idx, 1);
    }

    #[test]
    fn test_insert_action_enter_saves() {
        let mut s = state(&["hi"], &[]);
        s.mode = Mode::Insert(InsertMode::New);
        s.insert_action(InsertAction::Enter);
        assert!(matches!(s.mode, Mode::Normal));
    }

    #[test]
    fn test_insert_action_cancel_new_removes_item() {
        let mut s = state(&["a", "b"], &[]);
        s.mode = Mode::Insert(InsertMode::New);
        s.insert_action(InsertAction::Cancel);
        assert_eq!(s.todos, vec!["b"]);
        assert!(matches!(s.mode, Mode::Normal));
    }

    #[test]
    fn test_insert_action_cancel_edit_restores_snapshot() {
        let mut s = state(&["original"], &[]);
        s.mode = Mode::Insert(InsertMode::Edit("original".into()));
        // Simulate typing in insert mode
        let buf = s.todos.get_mut(0).unwrap();
        buf.push_str("+edited");
        assert_eq!(s.todos[0], "original+edited");
        // Cancel should restore
        s.insert_action(InsertAction::Cancel);
        assert_eq!(s.todos[0], "original");
        assert!(matches!(s.mode, Mode::Normal));
    }

    #[test]
    fn test_insert_action_char_appends() {
        let mut s = state(&["he"], &[]);
        s.mode = Mode::Insert(InsertMode::New);
        s.insert_action(InsertAction::Char('l'));
        s.insert_action(InsertAction::Char('l'));
        s.insert_action(InsertAction::Char('o'));
        assert_eq!(s.todos[0], "hello");
    }

    #[test]
    fn test_insert_action_delete_char_removes() {
        let mut s = state(&["hello"], &[]);
        s.mode = Mode::Insert(InsertMode::New);
        s.insert_action(InsertAction::DeleteChar);
        assert_eq!(s.todos[0], "hell");
    }

    #[test]
    fn test_start_edit_sets_edit_mode() {
        let mut s = state(&["editable"], &[]);
        s.start_edit();
        assert!(matches!(s.mode, Mode::Insert(InsertMode::Edit(_))));
    }

    #[test]
    fn test_start_edit_empty_list_does_nothing() {
        let mut s = state(&[], &[]);
        s.start_edit();
        assert!(matches!(s.mode, Mode::Normal));
    }

    #[test]
    fn test_start_edit_captures_snapshot() {
        let mut s = state(&["hello"], &[]);
        s.start_edit();
        if let Mode::Insert(InsertMode::Edit(snap)) = &s.mode {
            assert_eq!(snap, "hello");
        } else {
            panic!("expected Edit mode");
        }
    }

    #[test]
    fn test_show_help_sets_help_mode() {
        let mut s = state(&[], &[]);
        s.show_help();
        assert!(matches!(s.mode, Mode::Help));
    }

    #[test]
    fn test_toggle_show_number() {
        let mut s = state(&[], &[]);
        assert!(!s.show_number);
        s.toggle_show_number();
        assert!(s.show_number);
        s.toggle_show_number();
        assert!(!s.show_number);
    }

    #[test]
    fn test_handle_help_action_exits_with_q() {
        let mut s = state(&[], &[]);
        s.mode = Mode::Help;
        s.handle_help_action(&Action::SaveQuit);
        assert!(matches!(s.mode, Mode::Normal));
    }

    #[test]
    fn test_handle_help_action_exits_with_big_q() {
        let mut s = state(&[], &[]);
        s.mode = Mode::Help;
        s.handle_help_action(&Action::NoSaveQuit);
        assert!(matches!(s.mode, Mode::Normal));
    }

    #[test]
    fn test_handle_help_action_ignores_other() {
        let mut s = state(&[], &[]);
        s.mode = Mode::Help;
        s.handle_help_action(&Action::Enter);
        assert!(matches!(s.mode, Mode::Help));
    }

    #[test]
    fn test_get_current_buffer_returns_selected_item() {
        let s = state(&["first", "second"], &[]);
        assert_eq!(s.get_current_buffer().map(String::as_str), Some("first"));
    }

    #[test]
    fn test_get_current_buffer_out_of_bounds() {
        let mut s = state(&[], &[]);
        s.todos_idx = 0;
        assert!(s.get_current_buffer().is_none());
    }

    #[test]
    fn test_goto_list_pos_sets_index() {
        let mut s = state(&["a", "b", "c"], &[]);
        s.goto_list_pos(2);
        assert_eq!(s.todos_idx, 2);
    }

    #[test]
    fn test_goto_end_sets_max() {
        let mut s = state(&["a", "b", "c"], &[]);
        s.goto_list_pos(usize::MAX);
        assert_eq!(s.todos_idx, usize::MAX);
    }

    #[test]
    fn test_goto_list_pos_in_dones_tab() {
        let mut s = state(&[], &["x", "y", "z"]);
        s.curr_tab = Tab::Dones;
        s.goto_list_pos(1);
        assert_eq!(s.dones_idx, 1);
    }

    #[test]
    fn test_clamp_indexes_brings_out_of_bounds_down() {
        let mut s = state(&["a", "b"], &[]);
        s.todos_idx = 5;
        s.clamp_indexes();
        assert_eq!(s.todos_idx, 1);
    }

    #[test]
    fn test_clamp_indexes_empty_list_sets_zero() {
        let mut s = state(&[], &[]);
        s.todos_idx = 0;
        s.clamp_indexes();
        assert_eq!(s.todos_idx, 0);
    }

    #[test]
    fn test_clamp_indexes_dones() {
        let mut s = state(&[], &["x"]);
        s.curr_tab = Tab::Dones;
        s.dones_idx = 10;
        s.clamp_indexes();
        assert_eq!(s.dones_idx, 0);
    }

    #[test]
    fn test_clamp_indexes_valid_index_unchanged() {
        let mut s = state(&["a", "b", "c"], &[]);
        s.todos_idx = 1;
        s.clamp_indexes();
        assert_eq!(s.todos_idx, 1);
    }
}
