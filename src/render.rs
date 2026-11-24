use std::io::{self, BufWriter, Write};

use crossterm::{
    cursor, execute, queue, style,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

pub struct Renderer {
    out: BufWriter<io::Stdout>,
    size: (u16, u16),
}

impl Renderer {
    pub fn new() -> io::Result<Self> {
        let mut out = BufWriter::new(io::stdout());
        execute!(out, EnterAlternateScreen, cursor::Hide)?;
        terminal::enable_raw_mode()?;
        Ok(Self {
            out,
            size: terminal::size()?,
        })
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    pub fn set_size(&mut self, size: (u16, u16)) {
        self.size = size;
    }

    pub fn clear(&mut self) -> io::Result<()> {
        queue!(self.out, terminal::Clear(terminal::ClearType::All))
    }

    pub fn text(&mut self, pos: (u16, u16), text: &str, reverse: bool) -> io::Result<()> {
        self.cursor(pos)?;
        if reverse {
            queue!(
                self.out,
                style::SetAttribute(style::Attribute::Reverse),
                style::Print(text),
                style::SetAttribute(style::Attribute::NoReverse)
            )
        } else {
            queue!(self.out, style::Print(text))
        }
    }

    pub fn cursor(&mut self, (x, y): (u16, u16)) -> io::Result<()> {
        queue!(self.out, cursor::MoveTo(x, y))
    }

    pub fn present(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    pub fn reset(&mut self) -> io::Result<()> {
        self.out.flush()?;
        queue!(self.out, LeaveAlternateScreen, cursor::Show)?;
        self.out.flush()?;
        terminal::disable_raw_mode()?;
        Ok(())
    }
}

impl Drop for Renderer {
    /// Make sure to call reset_scr even if any fails happen in the main loop
    fn drop(&mut self) {
        if let Err(e) = self.reset() {
            eprintln!("Error while resetting screen: {e}");
        }
    }
}
