use std::{
    io, mem,
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
        if let Err(e) = self.main_loop() {
            match e.to_string().as_str() {
                globals::NO_SAVE => return Ok(()),
                globals::BREAK => {}
                _ => Err(e)?,
            }
        }
        save_to_file(
            &self.file_path,
            &self.curr_state.todos,
            &self.curr_state.dones,
        )?;
        Ok(())
    }
}

/// Main loop / Screen Drawing
impl App {
    fn main_loop(&mut self) -> anyhow::Result<()> {
        let _guard = ScreenGuard {};
        let mut term_size = Default::default();
        init_scr()?;
        loop {
            self.clamp_indexes();
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
                            self.handle_insert_mode(action);
                        }
                    }
                    Mode::Help => {
                        if let Ok(action) = Action::try_from(event) {
                            self.handle_help_mode(action);
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
    fn execute_action(&mut self, action: Action) -> anyhow::Result<()> {
        match action {
            Action::Enter => {
                self.push_state();
                self.handle_enter_press();
            }
            Action::SwitchTab(tab) => match tab {
                TabAction::Toggle => self.curr_state.curr_tab = self.curr_state.curr_tab.toggle(),
                TabAction::Left => self.curr_state.curr_tab = Tab::Todos,
                TabAction::Right => self.curr_state.curr_tab = Tab::Dones,
            },
            Action::Insert(direction) => {
                self.push_state();
                self.start_insert_mode(direction);
            }
            Action::Edit => {
                self.push_state();
                self.start_edit_mode();
            }
            Action::MoveCursor(direction) => self.handle_cursor_move(direction),
            Action::MoveItem(direction) => {
                self.push_state();
                self.handle_move_item(direction);
            }
            Action::GotoBegin => self.goto_list_pos(0),
            Action::GotoEnd => self.goto_list_pos(usize::MAX),
            Action::Delete => {
                self.push_state();
                self.handle_delete();
            }
            Action::SaveQuit => anyhow::bail!(globals::BREAK),
            Action::NoSaveQuit => anyhow::bail!(globals::NO_SAVE),
            Action::ShowHelp => self.curr_state.mode = Mode::Help,
            Action::ShowNumber => self.curr_state.show_number = !self.curr_state.show_number,
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
        }

        Ok(())
    }

    fn handle_enter_press(&mut self) {
        match self.curr_state.curr_tab {
            Tab::Todos => {
                if self.curr_state.todos.is_empty() {
                    return;
                }
                let value = self.curr_state.todos.remove(self.curr_state.todos_idx);
                self.curr_state.dones.push(value);
            }
            Tab::Dones => {
                if self.curr_state.dones.is_empty() {
                    return;
                }
                let value = self.curr_state.dones.remove(self.curr_state.dones_idx);
                self.curr_state.todos.push(value);
            }
        }
    }

    fn handle_cursor_move(&mut self, direction: KeyCode) {
        let idx = match self.curr_state.curr_tab {
            Tab::Todos => &mut self.curr_state.todos_idx,
            Tab::Dones => &mut self.curr_state.dones_idx,
        };
        match direction {
            KeyCode::Down => *idx += 1,
            KeyCode::Up => *idx = idx.saturating_sub(1),
            _ => unreachable!("This spot should't be reachable"),
        };
    }

    fn handle_delete(&mut self) {
        let (target_vec, idx) = match self.curr_state.curr_tab {
            Tab::Todos => (&mut self.curr_state.todos, &self.curr_state.todos_idx),
            Tab::Dones => (&mut self.curr_state.dones, &self.curr_state.dones_idx),
        };

        if target_vec.is_empty() {
            return;
        }

        target_vec.remove(*idx);
    }

    fn handle_move_item(&mut self, direction: KeyCode) {
        let (vec, idx) = match self.curr_state.curr_tab {
            Tab::Todos => (&mut self.curr_state.todos, &mut self.curr_state.todos_idx),
            Tab::Dones => (&mut self.curr_state.dones, &mut self.curr_state.dones_idx),
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

    fn handle_help_mode(&mut self, action: Action) {
        match action {
            Action::SaveQuit | Action::NoSaveQuit => self.curr_state.mode = Mode::Normal,
            _ => {}
        }
    }

    fn handle_insert_mode(&mut self, code: InsertAction) {
        let buf = match self.curr_state.curr_tab {
            Tab::Todos => self
                .curr_state
                .todos
                .get_mut(self.curr_state.todos_idx)
                .unwrap(),
            Tab::Dones => self
                .curr_state
                .dones
                .get_mut(self.curr_state.dones_idx)
                .unwrap(),
        };

        match code {
            InsertAction::Enter => self.disable_insert_mode(),
            InsertAction::Cancel => {
                match mem::replace(&mut self.curr_state.mode, Mode::Normal) {
                    Mode::Insert(InsertMode::Edit(snap)) => *buf = snap,
                    Mode::Insert(InsertMode::New) => self.handle_delete(),
                    _ => unreachable!(),
                };
                self.disable_insert_mode();
            }
            InsertAction::Char(c) => buf.push(c),
            InsertAction::DeleteChar => _ = buf.pop(),
        }
    }

    fn start_insert_mode(&mut self, direction: KeyCode) {
        self.curr_state.mode = Mode::Insert(InsertMode::New);

        let (list, idx) = match self.curr_state.curr_tab {
            Tab::Todos => (&mut self.curr_state.todos, &mut self.curr_state.todos_idx),
            Tab::Dones => (&mut self.curr_state.dones, &mut self.curr_state.dones_idx),
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

    fn start_edit_mode(&mut self) {
        let Some(snap) = self.get_current_buffer().cloned() else {
            return;
        };
        self.curr_state.mode = Mode::Insert(InsertMode::Edit(snap))
    }

    fn disable_insert_mode(&mut self) {
        self.curr_state.mode = Mode::Normal;
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

    fn get_current_buffer(&self) -> Option<&String> {
        match self.curr_state.curr_tab {
            Tab::Todos => self.curr_state.todos.get(self.curr_state.todos_idx),
            Tab::Dones => self.curr_state.dones.get(self.curr_state.dones_idx),
        }
    }

    fn goto_list_pos(&mut self, pos: usize) {
        let idx = match self.curr_state.curr_tab {
            Tab::Todos => &mut self.curr_state.todos_idx,
            Tab::Dones => &mut self.curr_state.dones_idx,
        };

        *idx = pos;
    }

    fn clamp_indexes(&mut self) {
        self.curr_state.todos_idx = self
            .curr_state
            .todos_idx
            .clamp(0, self.curr_state.todos.len().saturating_sub(1));
        self.curr_state.dones_idx = self
            .curr_state
            .dones_idx
            .clamp(0, self.curr_state.dones.len().saturating_sub(1));
    }
}
