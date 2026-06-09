use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum TabAction {
    Toggle,
    Left,
    Right,
}

pub enum Action {
    Enter,
    SwitchTab(TabAction),
    Insert(KeyCode),
    Edit,
    MoveCursor(KeyCode),
    MoveItem(KeyCode),
    GotoBegin,
    GotoEnd,
    Delete,
    SaveQuit,
    NoSaveQuit,
    ShowHelp,
    ShowNumber,
    Undo,
    Redo,
}

impl TryFrom<KeyEvent> for Action {
    type Error = ();

    fn try_from(event: KeyEvent) -> Result<Self, Self::Error> {
        use KeyCode::{Char, Down, Enter, F, Left, Right, Tab, Up};
        use KeyModifiers as M;
        use TabAction as TA;

        let m = event.modifiers;

        Ok(match event.code {
            // ── Core actions ────────────────────────────────
            Enter => Self::Enter,
            Tab => Self::SwitchTab(TA::Toggle),
            F(1) => Self::ShowHelp,
            Char('l') => Self::SwitchTab(TA::Right),
            Char('h') => Self::SwitchTab(TA::Left),
            Right if !m.contains(M::SHIFT) => Self::SwitchTab(TA::Right),
            Left if !m.contains(M::SHIFT) => Self::SwitchTab(TA::Left),
            Char('n') => Self::ShowNumber,

            // ── Insert / Edit ───────────────────────────────
            Char('i') => Self::Insert(Up),
            Char('o') => Self::Insert(Down),
            Char('e') => Self::Edit,

            // ── Navigation ─────────────────────────────────
            // Vim keys
            Char('k') => Self::MoveCursor(Up),
            Char('j') => Self::MoveCursor(Down),

            // Arrow keys
            Up if !m.contains(M::SHIFT) => Self::MoveCursor(Up),
            Down if !m.contains(M::SHIFT) => Self::MoveCursor(Down),

            // ── Item movement ──────────────────────────────
            // Vim keys
            Char('K') => Self::MoveItem(Up),
            Char('J') => Self::MoveItem(Down),

            // Shift + Arrows
            Up if m.contains(M::SHIFT) => Self::MoveItem(Up),
            Down if m.contains(M::SHIFT) => Self::MoveItem(Down),

            // ── Jumping ────────────────────────────────────
            Char('g') => Self::GotoBegin,
            Char('G') => Self::GotoEnd,

            // ── Undo / Redo ────────────────────────────────
            Char('u') => Self::Undo,
            Char('r') if m.contains(M::CONTROL) => Self::Redo,

            // ── Item / Buffer management ───────────────────
            Char('d') => Self::Delete,

            // ── Quit ───────────────────────────────────────
            Char('q') => Self::SaveQuit,
            Char('Q') => Self::NoSaveQuit,
            Char('c') if m.contains(M::CONTROL) => Self::NoSaveQuit,

            // ── Fallback ───────────────────────────────────
            _ => return Err(()),
        })
    }
}

pub enum InsertAction {
    Char(char),
    DeleteChar,
    Enter,
    Cancel,
}

impl TryFrom<KeyEvent> for InsertAction {
    type Error = ();

    fn try_from(event: KeyEvent) -> Result<Self, Self::Error> {
        use KeyCode::{Backspace, Char, Enter, Esc};

        Ok(match event.code {
            Char(c) => Self::Char(c),
            Backspace => Self::DeleteChar,
            Enter => Self::Enter,
            Esc => Self::Cancel,
            _ => return Err(()),
        })
    }
}
