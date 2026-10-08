//! The `options` of `Search`, validated. Every value's type is checked, unknown
//! keys are ignored, and a wrong type or an unknown value is refused with a
//! message the caller can show.

use atlas_file_index::category::Category;
use atlas_file_index::query::{KindFilter, Options};
use atlas_file_index::uri::uri_to_path;
use std::collections::HashMap;
use zbus::zvariant::{OwnedValue, Value};

/// Keys looked at; a caller sending more is refused.
const MAX_KEYS: usize = 64;
/// Entries accepted in `kinds`.
const MAX_KINDS: usize = 32;
/// Longest `tag`, in bytes (a tag name is at most 64 characters).
const MAX_TAG_BYTES: usize = 512;

/// A variant inside a variant is read as the inner one.
fn peel<'a, 'b>(mut v: &'a Value<'b>) -> &'a Value<'b> {
    while let Value::Value(inner) = v {
        v = inner;
    }
    v
}

fn as_str<'a>(v: &'a Value<'_>) -> Option<&'a str> {
    match peel(v) {
        Value::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

fn as_bool(v: &Value<'_>) -> Option<bool> {
    match peel(v) {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn as_i64(v: &Value<'_>) -> Option<i64> {
    match peel(v) {
        Value::I64(n) => Some(*n),
        _ => None,
    }
}

fn as_u64(v: &Value<'_>) -> Option<u64> {
    match peel(v) {
        Value::U64(n) => Some(*n),
        _ => None,
    }
}

fn as_strs<'a>(v: &'a Value<'_>) -> Option<Vec<&'a str>> {
    match peel(v) {
        Value::Array(a) => a.iter().map(as_str).collect(),
        _ => None,
    }
}

/// Validate the options of a `Search` call.
pub fn parse(options: &HashMap<String, OwnedValue>) -> Result<Options, String> {
    if options.len() > MAX_KEYS {
        return Err(format!(
            "too many options ({}, at most {MAX_KEYS})",
            options.len()
        ));
    }
    let mut o = Options::default();
    for (key, value) in options {
        let v: &Value<'_> = value;
        let bad = |want: &str| format!("option '{key}' must be {want}");
        match key.as_str() {
            "kind" => {
                o.kind = Some(match as_str(v).ok_or_else(|| bad("a string"))? {
                    "folder" => KindFilter::Folder,
                    "file" => KindFilter::File,
                    _ => return Err(bad("\"folder\" or \"file\"")),
                });
            }
            "kinds" => {
                let names = as_strs(v).ok_or_else(|| bad("an array of strings"))?;
                if names.len() > MAX_KINDS {
                    return Err(bad("an array of at most 32 names"));
                }
                let mut mask = 0u32;
                for n in names {
                    mask |= Category::from_name(n)
                        .ok_or_else(|| {
                            format!(
                                "option 'kinds' has an unknown category '{}'",
                                n.chars().take(40).collect::<String>()
                            )
                        })?
                        .bit();
                }
                if mask != 0 {
                    o.kinds = Some(mask);
                }
            }
            "root" => {
                let s = as_str(v).ok_or_else(|| bad("a string"))?;
                o.root = Some(uri_to_path(s).ok_or_else(|| bad("a file:// URI"))?);
            }
            "include_hidden" => o.include_hidden = as_bool(v).ok_or_else(|| bad("a boolean"))?,
            "modified_after" => {
                o.modified_after = Some(as_i64(v).ok_or_else(|| bad("a 64-bit integer (x)"))?)
            }
            "modified_before" => {
                o.modified_before = Some(as_i64(v).ok_or_else(|| bad("a 64-bit integer (x)"))?)
            }
            "size_min" => {
                o.size_min = Some(as_u64(v).ok_or_else(|| bad("an unsigned 64-bit integer (t)"))?)
            }
            "size_max" => {
                o.size_max = Some(as_u64(v).ok_or_else(|| bad("an unsigned 64-bit integer (t)"))?)
            }
            "match" => {
                o.path_match = match as_str(v).ok_or_else(|| bad("a string"))? {
                    "name" => false,
                    "path" => true,
                    _ => return Err(bad("\"name\" or \"path\"")),
                };
            }
            "tag" => {
                let s = as_str(v).ok_or_else(|| bad("a string"))?.trim();
                if s.is_empty() || s.len() > MAX_TAG_BYTES {
                    return Err(bad("a tag name (not empty, at most 512 bytes)"));
                }
                o.tag = Some(s.to_string());
            }
            _ => {} // unknown keys are ignored
        }
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(items: Vec<(&str, Value<'static>)>) -> HashMap<String, OwnedValue> {
        items
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.try_into().expect("no fds")))
            .collect()
    }

    #[test]
    fn accepts_every_documented_option() {
        let o = parse(&map(vec![
            ("kind", Value::from("file")),
            (
                "kinds",
                Value::new(vec!["image".to_string(), "pdf".to_string()]),
            ),
            ("root", Value::from("file:///home/a%20b")),
            ("include_hidden", Value::from(true)),
            ("modified_after", Value::from(100i64)),
            ("modified_before", Value::from(200i64)),
            ("size_min", Value::from(5u64)),
            ("size_max", Value::from(6u64)),
            ("match", Value::from("path")),
            ("tag", Value::from("  Red ")),
            ("unknown", Value::from(1u8)),
        ]))
        .unwrap();
        assert_eq!(o.kind, Some(KindFilter::File));
        assert_eq!(o.kinds, Some(Category::Image.bit() | Category::Pdf.bit()));
        assert_eq!(o.root, Some(b"/home/a b".to_vec()));
        assert!(o.include_hidden && o.path_match);
        assert_eq!(o.tag.as_deref(), Some("Red"));
        assert_eq!(
            (o.modified_after, o.modified_before, o.size_min, o.size_max),
            (Some(100), Some(200), Some(5), Some(6))
        );
    }

    #[test]
    fn empty_is_default() {
        assert_eq!(parse(&HashMap::new()).unwrap(), Options::default());
    }

    #[test]
    fn wrong_types_and_values_are_refused_in_plain_words() {
        for (k, v) in [
            ("kind", Value::from(1i64)),
            ("kind", Value::from("directory")),
            ("kinds", Value::from("image")),
            ("kinds", Value::new(vec!["nonsense".to_string()])),
            ("kinds", Value::new(vec![1i64])),
            ("root", Value::from("/not/a/uri")),
            ("root", Value::from(5u32)),
            ("include_hidden", Value::from("yes")),
            ("modified_after", Value::from(1u64)),
            ("modified_before", Value::from("x")),
            ("size_min", Value::from(-1i64)),
            ("size_max", Value::from(1.5f64)),
            ("match", Value::from("both")),
            ("tag", Value::from(5u32)),
            ("tag", Value::from(vec!["a".to_string()])),
            ("tag", Value::from("")),
            ("tag", Value::from("   ")),
            ("tag", Value::from("x".repeat(600))),
        ] {
            let e = parse(&map(vec![(k, v)])).expect_err(k);
            assert!(
                e.contains(k) && e.contains("must be") || e.contains("unknown category"),
                "{e}"
            );
        }
    }

    #[test]
    fn nested_variants_are_read() {
        let inner = Value::from(true);
        let o = parse(&map(vec![(
            "include_hidden",
            Value::Value(Box::new(inner)),
        )]))
        .unwrap();
        assert!(o.include_hidden);
    }

    #[test]
    fn too_many_keys() {
        let items: Vec<(String, Value<'static>)> = (0..100)
            .map(|i| (format!("k{i}"), Value::from(1u8)))
            .collect();
        let m: HashMap<String, OwnedValue> = items
            .into_iter()
            .map(|(k, v)| (k, v.try_into().unwrap()))
            .collect();
        assert!(parse(&m).is_err());
    }
}
