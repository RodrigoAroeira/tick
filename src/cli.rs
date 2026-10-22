use anyhow::Context;
use clap::Parser;
use dirs::home_dir;

use std::path::PathBuf;

#[derive(Parser)]
#[command(version)]
pub struct Args {
    #[arg(value_parser=path_parser)]
    path: Option<PathBuf>,
}

impl Args {
    pub fn path(&self) -> anyhow::Result<PathBuf> {
        let mut path = match self.path {
            Some(ref p) => p.clone(),
            None => home_dir()
            .context("Unable to find home directory")?
        };

        if path.is_dir() {
            path = path.join("TODO");
        }
        Ok(path)
    }
}

fn path_parser(path: &str) -> anyhow::Result<PathBuf> {
    if path.is_empty() {
        anyhow::bail!("Path can't be empty");
    }
    Ok(PathBuf::from(path))
}
