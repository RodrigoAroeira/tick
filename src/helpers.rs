use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

use anyhow::Result;
// TODO: Replace this dependency with builtin logic
use unicode_width::UnicodeWidthChar;

use crate::globals;

pub fn get_todos_dones<P>(path: P) -> Result<(Vec<String>, Vec<String>)>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    if path.is_dir() {
        anyhow::bail!("Provided path is a directory: {:?}", path.display());
    }

    let _ = std::fs::remove_file(path.with_extension("tmp"));

    let Ok(file) = File::open(path) else {
        return Ok(Default::default());
    };

    let reader = io::BufReader::new(file);
    let mut todos = Vec::new();
    let mut dones = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if let Some(s) = line.strip_prefix(globals::TODO_PREFIX) {
            todos.push(s.to_string());
        } else if let Some(s) = line.strip_prefix(globals::DONE_PREFIX) {
            dones.push(s.to_string());
        } else {
            anyhow::bail!("Malformed line: {:?}", line);
        }
    }

    Ok((todos, dones))
}

pub fn split_to_fit(
    s: &str,
    max_width: usize,
    offset: usize, // width to skip at the beginning (like line_begin + " ")
) -> (&str, Vec<&str>) {
    let mut width = 0;

    for (i, c) in s.char_indices() {
        let cw = c.width().unwrap_or(1);

        if width + cw > max_width {
            if i == 0 {
                let (first_char, remainder) = s.split_at(c.len_utf8());
                return (first_char, split_remainder(remainder, max_width, offset));
            }

            let (first_part, remainder) = s.split_at(i);
            return (first_part, split_remainder(remainder, max_width, offset));
        }

        width += cw;
    }

    (s, Vec::new())
}

fn split_remainder(s: &str, max_width: usize, offset: usize) -> Vec<&str> {
    let mut result = Vec::new();
    let mut current = s;

    while !current.is_empty() {
        let mut width = offset;
        let mut split_index = current.len();

        for (i, c) in current.char_indices() {
            let cw = c.width().unwrap_or(1);

            if width + cw > max_width {
                split_index = i;
                break;
            }

            width += cw;
        }

        let (chunk, remainder) = current.split_at(split_index);
        result.push(chunk);
        current = remainder;
    }

    result
}
