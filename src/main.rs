use crate::app::App;

use clap::Parser;

mod action;
mod app;
mod cli;
mod globals;
mod helpers;
mod screen_guard;
mod tab;

fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();
    let mut app = App::new(args.path()?)?;

    app.run()
}
