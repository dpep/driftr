//! A capped, append-only log of every hook run, for judging what hooks are worth.
//! Lives in the data dir (not a repo) so it also records runs that did nothing.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::Value;

use crate::external;

/// Past this, the oldest half is dropped. Newest lines matter most when diagnosing.
const CAP_BYTES: u64 = 5 * 1024 * 1024;

pub fn path() -> Result<PathBuf> {
    Ok(external::data_dir()?.join("hooks.jsonl"))
}

/// Appends one record as a single write, so concurrent hooks don't interleave lines.
pub fn append(record: &Value) -> Result<()> {
    let path = path()?;
    fs::create_dir_all(path.parent().unwrap())?;
    let mut line = record.to_string();
    line.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?
        .write_all(line.as_bytes())?;
    if fs::metadata(&path)?.len() > CAP_BYTES {
        trim(&path)?;
    }
    Ok(())
}

fn trim(path: &PathBuf) -> Result<()> {
    let text = fs::read_to_string(path)?;
    let half = text.len() / 2;
    // Resume at a line boundary so the first kept record is whole.
    let start = text[half..].find('\n').map_or(text.len(), |i| half + i + 1);
    let tmp = path.with_extension("jsonl.tmp");
    fs::write(&tmp, &text[start..])?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn read() -> Result<Vec<Value>> {
    let Ok(text) = fs::read_to_string(path()?) else {
        return Ok(vec![]);
    };
    Ok(text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}
