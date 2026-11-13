//! Masks and positions saved into a take's session file.

use super::take_io::folder_layout;
use super::*;
use timegrapher_core::take_context::{in_files, FilePart};

/// Save `masked` to the entries for `path` in its take's session file,
/// leaving the rest of the file as it was, comments and keys this app
/// doesn't read too, and with `positions` (start, end and position
/// code of each stretch, as [`App::opened_entries`] gives them) put one
/// entry per stretch in place of those entries, each keeping what the
/// first of them said besides. Positions are refused where the entries
/// list more than one file.
pub(super) fn save_marks_to_take(
    path: &Path,
    masked: &[[f64; 2]],
    positions: Option<&Entries>,
    recorded: &[RecordedPosition],
) -> Result<PathBuf, String> {
    let dir = take_dir(path)
        .ok_or_else(|| format!("{} has no folder", path.display()))?
        .to_path_buf();
    let file = dir.join(MANIFEST);
    let text = timegrapher_core::recorder::read_settled(&file)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("Can't read {}: {e}", file.display()))?;
    let listed = |t: &dyn toml_edit::TableLike| {
        t.get("file")
            .and_then(|f| f.as_str())
            .is_some_and(|f| lists(&dir, f, path))
    };
    let one_file = |files: Vec<String>| {
        if files.windows(2).any(|w| w[0] != w[1]) {
            Err(format!(
                "{} lists {} as more than one file, so its positions can't be saved",
                file.display(),
                capture_label(path)
            ))
        } else {
            Ok(())
        }
    };
    let files = read_manifest(path)
        .ok()
        .flatten()
        .and_then(|(m, entries)| folder_layout(path, &m, &entries))
        .map(|l| l.0);
    let marks = Marks {
        listed: &listed,
        one_file: &one_file,
        masked,
        dir: &dir,
        files: files.as_deref(),
    };
    let changed = match doc.get_mut("recording") {
        Some(toml_edit::Item::ArrayOfTables(tables)) => marks.in_tables(tables, positions)?,
        Some(toml_edit::Item::Value(toml_edit::Value::Array(entries))) => {
            marks.in_inline(entries, positions)?
        }
        _ => 0,
    };
    if changed == 0 {
        return Err(format!(
            "{} doesn't list {}",
            file.display(),
            capture_label(path)
        ));
    }
    // With the positions, what was recorded under any painting of them.
    if positions.is_some() {
        set_recorded(&mut doc, recorded);
    }
    write_replacing(&file, &doc.to_string())?;
    Ok(file)
}

/// What [`save_marks_to_take`] puts in a session file's entries.
struct Marks<'a> {
    /// Whether an entry is one for the take's file.
    listed: &'a dyn Fn(&dyn toml_edit::TableLike) -> bool,
    /// Refuses positions where the entries list more than one file.
    one_file: &'a dyn Fn(Vec<String>) -> Result<(), String>,
    masked: &'a [[f64; 2]],
    /// The take's folder, and where each of its sound files sits on the
    /// strip when it has several: an entry for one of them keeps the masks
    /// over it, in seconds into it (#1154).
    dir: &'a Path,
    files: Option<&'a [FilePart]>,
}

impl Marks<'_> {
    /// An entry's masked stretches, or none.
    fn set(&self, t: &mut dyn toml_edit::TableLike) {
        let masked = self.masked_in(&file_of(t));
        if masked.is_empty() {
            t.remove("masked");
        } else {
            let mut spans = toml_edit::Array::new();
            for m in masked {
                spans.push(toml_edit::Array::from_iter([m[0], m[1]]));
            }
            t.insert("masked", toml_edit::value(spans));
        }
    }

    /// The masks an entry naming `file` keeps: for one file of a folder
    /// take, those over it, cut at its ends and in seconds into it; else
    /// every mask, in the strip's seconds.
    fn masked_in(&self, file: &str) -> Vec<[f64; 2]> {
        let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        let named = self.dir.join(file);
        let at = self
            .files
            .and_then(|f| Some((f, f.iter().position(|p| same(&p.path, &named))?)));
        match at {
            Some((files, i)) => self
                .masked
                .iter()
                .flat_map(|m| in_files(files, m[0], m[1]))
                .filter(|&(j, _, _)| j == i)
                .map(|(_, a, b)| [a, b])
                .collect(),
            None => self.masked.to_vec(),
        }
    }

    /// The marks in `[[recording]]` tables; how many entries they changed.
    fn in_tables(
        &self,
        tables: &mut toml_edit::ArrayOfTables,
        positions: Option<&Entries>,
    ) -> Result<usize, String> {
        let mut changed = 0;
        let Some(positions) = positions else {
            for t in tables.iter_mut().filter(|t| (self.listed)(*t)) {
                self.set(t);
                changed += 1;
            }
            return Ok(changed);
        };
        (self.one_file)(
            tables
                .iter()
                .filter(|t| (self.listed)(*t))
                .map(|t| file_of(t))
                .collect(),
        )?;
        let mut out = toml_edit::ArrayOfTables::new();
        let mut done = false;
        for t in tables.iter() {
            if !(self.listed)(t) {
                out.push(t.clone());
                continue;
            }
            changed += 1;
            if std::mem::replace(&mut done, true) {
                continue;
            }
            for (i, p) in positions.iter().enumerate() {
                let mut e = t.clone();
                // A comment stays on the first copy alone.
                if i > 0 {
                    e.decor_mut().set_prefix("\n");
                    for (_, item) in e.iter_mut() {
                        if let Some(v) = item.as_value_mut() {
                            v.decor_mut().clear();
                        }
                    }
                }
                place_position(&mut e, p);
                self.set(&mut e);
                out.push(e);
            }
        }
        *tables = out;
        Ok(changed)
    }

    /// The marks in a `recording = [...]` array of inline tables; how many
    /// entries they changed.
    fn in_inline(
        &self,
        entries: &mut toml_edit::Array,
        positions: Option<&Entries>,
    ) -> Result<usize, String> {
        let mut changed = 0;
        let Some(positions) = positions else {
            for t in entries.iter_mut().filter_map(|v| v.as_inline_table_mut()) {
                if (self.listed)(t) {
                    self.set(t);
                    changed += 1;
                }
            }
            return Ok(changed);
        };
        (self.one_file)(
            entries
                .iter()
                .filter_map(|v| v.as_inline_table())
                .filter(|t| (self.listed)(*t))
                .map(|t| file_of(t))
                .collect(),
        )?;
        let mut out = toml_edit::Array::new();
        let mut done = false;
        for v in entries.iter() {
            let Some(t) = v.as_inline_table().filter(|t| (self.listed)(*t)) else {
                out.push_formatted(v.clone());
                continue;
            };
            changed += 1;
            if std::mem::replace(&mut done, true) {
                continue;
            }
            for p in positions {
                let mut e = t.clone();
                place_position(&mut e, p);
                self.set(&mut e);
                out.push(e);
            }
        }
        *entries = out;
        Ok(changed)
    }
}

/// The file an entry names.
fn file_of(t: &dyn toml_edit::TableLike) -> String {
    t.get("file")
        .and_then(|f| f.as_str())
        .unwrap_or_default()
        .to_string()
}

/// One stretch's entry: the first entry listed, with its position and
/// times.
fn place_position(
    t: &mut dyn toml_edit::TableLike,
    (start, end, code): &(Option<f64>, Option<f64>, String),
) {
    t.insert("position", toml_edit::value(code.as_str()));
    for (key, v) in [("start_s", start), ("end_s", end)] {
        match v {
            Some(v) => t.insert(key, toml_edit::value(*v)),
            None => t.remove(key),
        };
    }
    // Each start is already after its settling: nothing to skip again.
    t.insert("settle_s", toml_edit::value(0.0));
}

/// What was recorded under any painting of the positions, or none.
fn set_recorded(doc: &mut toml_edit::DocumentMut, recorded: &[RecordedPosition]) {
    if recorded.is_empty() {
        doc.remove("recorded");
        return;
    }
    let mut out = toml_edit::ArrayOfTables::new();
    for r in recorded {
        let mut t = toml_edit::Table::new();
        t.insert("position", toml_edit::value(r.position.as_str()));
        for (key, v) in [("start_s", r.start_s), ("end_s", r.end_s)] {
            if let Some(v) = v {
                t.insert(key, toml_edit::value(v));
            }
        }
        out.push(t);
    }
    doc.insert("recorded", toml_edit::Item::ArrayOfTables(out));
}
