//! One renderer for every command, so `-j`/`-J` mean the same thing everywhere.

use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    Text,
    Json,
    Ndjson,
}

impl Format {
    /// A list answer: text lines, a pretty JSON array, or one compact object per line.
    pub fn rows<T: Serialize>(self, rows: &[T], text: impl Fn(&T) -> String) -> Result<()> {
        match self {
            Format::Text => rows.iter().for_each(|r| println!("{}", text(r))),
            Format::Json => println!("{}", serde_json::to_string_pretty(rows)?),
            Format::Ndjson => {
                for r in rows {
                    println!("{}", serde_json::to_string(r)?);
                }
            }
        }
        Ok(())
    }

    /// A list answer whose text form is a table rendered as a whole.
    pub fn rows_or_table<T: Serialize>(
        self,
        rows: &[T],
        table: impl Fn(&[T]) -> String,
    ) -> Result<()> {
        match self {
            Format::Text => print!("{}", table(rows)),
            _ => self.rows(rows, |_| String::new())?,
        }
        Ok(())
    }

    /// A single answer; `text` returning `None` prints nothing in text mode.
    pub fn one<T: Serialize>(
        self,
        value: &T,
        text: impl FnOnce(&T) -> Option<String>,
    ) -> Result<()> {
        match self {
            Format::Text => {
                if let Some(s) = text(value) {
                    println!("{s}");
                }
            }
            Format::Json => println!("{}", serde_json::to_string_pretty(value)?),
            Format::Ndjson => println!("{}", serde_json::to_string(value)?),
        }
        Ok(())
    }

    /// A pretty array can't stream, so long-running output degrades `-j` to one object per line.
    pub fn streaming(self) -> Format {
        match self {
            Format::Json => Format::Ndjson,
            f => f,
        }
    }
}
