//! Data files for Loop nodes (plan 13c): a `.csv` or `.json` file of the collection, one
//! loop iteration per row. Nothing outside the collection is read, and size and row
//! counts are capped.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

use super::model::{is_safe_relative, MAX_LOOP_ITEMS};

pub type Row = BTreeMap<String, String>;

/// Largest data file read.
pub const MAX_DATA_BYTES: u64 = 1024 * 1024;
const MAX_COLUMNS: usize = 100;

fn column_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Splits CSV text into records (RFC 4180: quotes, `""`, CRLF); blank lines are skipped.
fn records(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut all = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    let mut chars = text.chars().peekable();
    let end_record = |record: &mut Vec<String>,
                      field: &mut String,
                      all: &mut Vec<Vec<String>>,
                      was_quoted: bool| {
        record.push(std::mem::take(field));
        let blank = record.len() == 1 && record[0].is_empty() && !was_quoted;
        let done = std::mem::take(record);
        if !blank {
            all.push(done);
        }
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                quoted = true;
                was_quoted = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                end_record(&mut record, &mut field, &mut all, was_quoted);
                was_quoted = false;
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return Err("A quoted value is not closed.".into());
    }
    if !field.is_empty() || !record.is_empty() || was_quoted {
        end_record(&mut record, &mut field, &mut all, was_quoted);
    }
    Ok(all)
}

fn parse_csv(text: &str) -> Result<Vec<Row>, String> {
    let mut lines = records(text.trim_start_matches('\u{feff}'))?.into_iter();
    let Some(header) = lines.next() else {
        return Ok(Vec::new());
    };
    let names: Vec<String> = header.iter().map(|name| name.trim().to_string()).collect();
    if names.len() > MAX_COLUMNS {
        return Err(format!("More than {MAX_COLUMNS} columns."));
    }
    if let Some(bad) = names.iter().find(|name| !column_ok(name)) {
        return Err(format!("The column name \"{bad}\" is not a variable name."));
    }
    let mut seen = std::collections::HashSet::new();
    if let Some(twice) = names.iter().find(|name| !seen.insert(name.as_str())) {
        return Err(format!("The column \"{twice}\" appears twice."));
    }
    let mut rows = Vec::new();
    for (n, line) in lines.enumerate() {
        if line.len() != names.len() {
            return Err(format!(
                "Row {} has {} values for {} columns.",
                n + 2,
                line.len(),
                names.len()
            ));
        }
        rows.push(names.iter().cloned().zip(line).collect());
    }
    Ok(rows)
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn parse_json(text: &str) -> Result<Vec<Row>, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("Not valid JSON: {e}."))?;
    let Value::Array(items) = value else {
        return Err("The JSON file must be an array of objects.".into());
    };
    items
        .iter()
        .enumerate()
        .map(|(n, item)| {
            let Value::Object(fields) = item else {
                return Err(format!("Item {} is not an object.", n + 1));
            };
            if fields.len() > MAX_COLUMNS {
                return Err(format!(
                    "Item {} has more than {MAX_COLUMNS} fields.",
                    n + 1
                ));
            }
            if let Some(bad) = fields.keys().find(|key| !column_ok(key)) {
                return Err(format!("The field name \"{bad}\" is not a variable name."));
            }
            Ok(fields
                .iter()
                .map(|(key, value)| (key.clone(), text_of(value)))
                .collect())
        })
        .collect()
}

/// Rows of a data file's text; the kind comes from the file name.
pub fn parse(file: &str, text: &str) -> Result<Vec<Row>, String> {
    let rows = if file.to_ascii_lowercase().ends_with(".json") {
        parse_json(text)?
    } else {
        parse_csv(text)?
    };
    if rows.len() > MAX_LOOP_ITEMS {
        return Err(format!("More than {MAX_LOOP_ITEMS} rows."));
    }
    Ok(rows)
}

/// Reads the data file `file` (relative to the collection `root`).
pub fn read_rows(root: &Path, file: &str) -> Result<Vec<Row>, String> {
    if !is_safe_relative(file) {
        return Err("The data file is not a path inside the collection.".into());
    }
    let inside = |path: &Path| -> Result<std::path::PathBuf, String> {
        path.canonicalize()
            .map_err(|_| format!("Could not open the data file {file}."))
    };
    let base = inside(root)?;
    let path = inside(&root.join(file))?;
    if !path.starts_with(&base) {
        return Err("The data file is not a path inside the collection.".into());
    }
    // One handle for the check and the read, and a bounded read: the file may change
    // between the two otherwise.
    let handle = std::fs::File::open(&path).map_err(|_| format!("Could not open {file}."))?;
    let meta = handle
        .metadata()
        .map_err(|_| format!("Could not open {file}."))?;
    if !meta.is_file() || meta.len() > MAX_DATA_BYTES {
        return Err(format!("{file} is not a file, or is over 1 MiB."));
    }
    let mut bytes = Vec::new();
    handle
        .take(MAX_DATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("Could not read {file}."))?;
    if bytes.len() as u64 > MAX_DATA_BYTES {
        return Err(format!("{file} is not a file, or is over 1 MiB."));
    }
    let text = String::from_utf8(bytes).map_err(|_| format!("{file} is not UTF-8 text."))?;
    parse(file, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, &str)]) -> Row {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn reads_csv_with_quotes_crlf_and_a_bom() {
        let text = "\u{feff}name,note\r\nann,\"hi, there\"\r\n\r\nbob,\"say \"\"x\"\"\"\r\ncy,\"two\nlines\"";
        assert_eq!(
            parse("u.csv", text).unwrap(),
            vec![
                row(&[("name", "ann"), ("note", "hi, there")]),
                row(&[("name", "bob"), ("note", "say \"x\"")]),
                row(&[("name", "cy"), ("note", "two\nlines")]),
            ]
        );
    }

    #[test]
    fn keeps_empty_values_and_a_last_row_without_a_newline() {
        let rows = parse("u.csv", "a,b\n1,\n,2").unwrap();
        assert_eq!(
            rows,
            vec![row(&[("a", "1"), ("b", "")]), row(&[("a", ""), ("b", "2")])]
        );
        assert!(parse("u.csv", "").unwrap().is_empty());
        assert!(parse("u.csv", "a,b\n").unwrap().is_empty());
    }

    #[test]
    fn rejects_broken_csv() {
        assert!(parse("u.csv", "a,b\n1").unwrap_err().contains("Row 2"));
        assert!(parse("u.csv", "a,b\n\"1,2")
            .unwrap_err()
            .contains("not closed"));
        assert!(parse("u.csv", "a,a\n1,2").unwrap_err().contains("twice"));
        assert!(parse("u.csv", "a b,c\n1,2")
            .unwrap_err()
            .contains("variable name"));
    }

    #[test]
    fn reads_json_arrays_of_objects() {
        let rows = parse(
            "u.json",
            r#"[{"id":1,"ok":true,"n":null,"o":{"a":1},"s":"x"}]"#,
        )
        .unwrap();
        assert_eq!(
            rows,
            vec![row(&[
                ("id", "1"),
                ("ok", "true"),
                ("n", ""),
                ("o", "{\"a\":1}"),
                ("s", "x")
            ])]
        );
        assert!(parse("u.json", "{}").is_err());
        assert!(parse("u.json", "[1]").is_err());
        assert!(parse("u.json", "[{\"a b\":1}]").is_err());
        assert!(parse("u.json", "nope").is_err());
    }

    #[test]
    fn caps_the_number_of_rows() {
        let text = format!("a\n{}", "1\n".repeat(MAX_LOOP_ITEMS + 1));
        assert!(parse("u.csv", &text).unwrap_err().contains("rows"));
        let text = format!("a\n{}", "1\n".repeat(MAX_LOOP_ITEMS));
        assert_eq!(parse("u.csv", &text).unwrap().len(), MAX_LOOP_ITEMS);
    }

    #[test]
    fn reads_files_inside_the_collection_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("coll");
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/u.csv"), "a\n1\n").unwrap();
        std::fs::write(dir.path().join("secret.csv"), "a\nx\n").unwrap();
        assert_eq!(read_rows(&root, "data/u.csv").unwrap().len(), 1);
        assert!(read_rows(&root, "../secret.csv").is_err());
        assert!(read_rows(&root, "data/none.csv").is_err());
        std::fs::write(
            root.join("big.csv"),
            vec![b'a'; (MAX_DATA_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(read_rows(&root, "big.csv").unwrap_err().contains("1 MiB"));
        std::fs::write(root.join("bin.csv"), [0xff, 0xfe]).unwrap();
        assert!(read_rows(&root, "bin.csv").unwrap_err().contains("UTF-8"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_collection_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("coll");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(dir.path().join("secret.csv"), "a\nx\n").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret.csv"), root.join("link.csv")).unwrap();
        assert!(read_rows(&root, "link.csv").is_err());
    }
}
