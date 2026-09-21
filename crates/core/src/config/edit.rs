//! Changing one value in a TOML file without rewriting the rest.
//!
//! Serialising a parsed struct back out is the obvious way to save a change
//! and the wrong one: comments are not fields, so they vanish, and the field
//! order becomes the struct's rather than the author's. These files are meant
//! to be read and edited by hand — losing what someone wrote in them is worse
//! than the edit is worth.

use toml_edit::{Array, DocumentMut, Item, Value};

use crate::error::{Error, Result};

fn parse(raw: &str) -> Result<DocumentMut> {
    raw.parse::<DocumentMut>()
        .map_err(|e| Error::Config(format!("parsing toml: {e}")))
}

/// Walk to the table `path` points at, creating what is missing.
fn table<'a>(doc: &'a mut DocumentMut, path: &[&str]) -> Result<&'a mut Item> {
    let mut item: &mut Item = doc.as_item_mut();
    for key in path {
        if item.get(key).is_none() {
            item[key] = toml_edit::table();
        }
        item = item
            .get_mut(key)
            .ok_or_else(|| Error::Config(format!("cannot reach `{key}` in toml")))?;
    }
    Ok(item)
}

/// Replace an array of strings, keeping the document's formatting.
pub fn put_strings(raw: &str, path: &[&str], key: &str, values: &[String]) -> Result<String> {
    let mut doc = parse(raw)?;
    let mut array = Array::new();
    for value in values {
        array.push(value.as_str());
    }
    // Multi-line, matching how these lists are written by hand and by the
    // scaffold — a package list on one line is unreadable at any real size.
    array.iter_mut().for_each(|v| {
        v.decor_mut().set_prefix("\n    ");
    });
    array.set_trailing("\n");
    array.set_trailing_comma(true);

    table(&mut doc, path)?[key] = Item::Value(Value::Array(array));
    Ok(doc.to_string())
}

/// Replace a string value, or remove the key when `value` is `None`.
pub fn put_string(raw: &str, path: &[&str], key: &str, value: Option<&str>) -> Result<String> {
    let mut doc = parse(raw)?;
    let item = table(&mut doc, path)?;
    match value {
        Some(value) => item[key] = toml_edit::value(value),
        None => {
            item.as_table_like_mut()
                .ok_or_else(|| Error::Config("not a table".into()))?
                .remove(key);
        }
    }
    Ok(doc.to_string())
}

/// Append an entry to an array of tables (`[[requires]]`), leaving the rest
/// of the document alone.
///
/// Pairs are written in the order given, because these files are read by
/// people and `name` first reads better than whatever a map iteration
/// happens to produce.
pub fn push_table(raw: &str, key: &str, pairs: &[(&str, &str)]) -> Result<String> {
    let mut doc = parse(raw)?;
    let mut entry = toml_edit::Table::new();
    for (k, v) in pairs {
        entry[k] = toml_edit::value(*v);
    }

    let existing = doc.get_mut(key).and_then(|i| i.as_array_of_tables_mut());
    match existing {
        Some(array) => array.push(entry),
        None => {
            let mut array = toml_edit::ArrayOfTables::new();
            array.push(entry);
            doc[key] = Item::ArrayOfTables(array);
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SET: &str = r#"# what this set is for
description = "Base"

[packages]
# the ones everyone needs
brew = ["alpha", "bravo"]
"#;

    #[test]
    fn replacing_an_array_keeps_every_comment() {
        let out = put_strings(SET, &["packages"], "brew", &["charlie".into()]).unwrap();
        assert!(out.contains("# what this set is for"), "{out}");
        assert!(out.contains("# the ones everyone needs"), "{out}");
        assert!(out.contains("charlie"), "{out}");
        assert!(!out.contains("alpha"), "{out}");
    }

    #[test]
    fn it_does_not_reorder_what_it_did_not_touch() {
        let out = put_strings(SET, &["packages"], "brew", &["charlie".into()]).unwrap();
        let desc = out.find("description").unwrap();
        let packages = out.find("[packages]").unwrap();
        assert!(desc < packages, "author order survives:\n{out}");
    }

    #[test]
    fn a_removed_key_is_gone_and_the_rest_remains() {
        let raw = "# keep me\nsecret_provider = \"1password\"\nvault = \"Private\"\n";
        let out = put_string(raw, &[], "vault", None).unwrap();
        assert!(!out.contains("vault"), "{out}");
        assert!(out.contains("# keep me"), "{out}");
        assert!(out.contains("1password"), "{out}");
    }

    #[test]
    fn a_missing_table_is_created_rather_than_failing() {
        let out = put_strings(
            "description = \"x\"\n",
            &["packages"],
            "cask",
            &["figma".into()],
        )
        .unwrap();
        assert!(out.contains("[packages]"), "{out}");
        assert!(out.contains("figma"), "{out}");
    }

    #[test]
    fn a_new_entry_is_appended_without_disturbing_what_is_there() {
        let raw = "# top\ndescription = \"x\"\n\n[[requires]]\nname = \"nvm\"\n";
        let out = push_table(
            raw,
            "requires",
            &[("name", "oh-my-zsh"), ("path", "~/.oh-my-zsh")],
        )
        .unwrap();

        assert!(out.contains("# top"), "{out}");
        assert!(out.contains("nvm"), "{out}");
        assert!(out.contains("oh-my-zsh"), "{out}");
        assert_eq!(out.matches("[[requires]]").count(), 2, "{out}");
        // `name` first: these are read by people.
        assert!(
            out.rfind("name").unwrap() < out.rfind("path").unwrap(),
            "{out}"
        );
    }

    #[test]
    fn the_array_is_created_when_the_file_has_none() {
        let out = push_table("description = \"x\"\n", "requires", &[("name", "nvm")]).unwrap();
        assert!(out.contains("[[requires]]"), "{out}");
    }
}
