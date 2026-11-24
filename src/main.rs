use clap::Parser;

use crate::app::App;

mod action;
mod app;
mod cli;
mod globals;
mod helpers;
mod render;
mod tab;

fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();
    let mut app = App::new(args.path()?)?;

    app.run()
}
