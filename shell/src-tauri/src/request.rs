//! Which requests the panel may send to the core (see core/src/ipc.rs for all of them).
//! The panel sends a turn (`text`), the answer to a held action (`confirm`, `cancel`) and the
//! calendar's `upcoming` and `delete_event`. Anything else, such as `hardware`, which only the
//! installer uses, is refused here instead of being forwarded.

use serde_json::Value;

/// Each kind the panel sends, with the extra fields that go with it.
const KINDS: [(&str, &[&str]); 5] = [
    ("text", &["language"]),
    ("confirm", &["time"]),
    ("cancel", &[]),
    ("upcoming", &["from"]),
    ("delete_event", &[]),
];

pub fn check(line: &str) -> Result<(), String> {
    let Ok(Value::Object(request)) = serde_json::from_str::<Value>(line) else {
        return Err("the request is not a JSON object".into());
    };
    let allowed = || KINDS.iter().map(|(kind, _)| format!("`{kind}`")).collect::<Vec<_>>().join(", ");
    let kinds: Vec<_> = KINDS.iter().filter(|(kind, _)| request.contains_key(*kind)).collect();
    let one_kind = || format!("a request has exactly one kind of {}", allowed());
    if kinds.len() > 1 {
        return Err(one_kind());
    }
    let (kind, extras) = kinds.first().map_or(("", &[][..]), |(kind, extras)| (*kind, *extras));
    if let Some(key) = request.keys().find(|key| *key != "id" && *key != kind && !extras.contains(&key.as_str())) {
        return Err(format!("`{key}` is not a request the panel may send; the kinds are {}", allowed()));
    }
    if kinds.is_empty() {
        return Err(one_kind());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_request_the_panel_sends_is_allowed() {
        for line in [
            r#"{"id":1,"text":"hi","language":"en"}"#,
            r#"{"id":2,"text":"مرحبا","language":"ar"}"#,
            r#"{"id":3,"confirm":"a1"}"#,
            r#"{"id":4,"confirm":"a1","time":"2026-01-02T09:00"}"#,
            r#"{"id":5,"cancel":"a1"}"#,
            r#"{"id":6,"upcoming":true}"#,
            r#"{"id":7,"upcoming":true,"from":"2026-01-02"}"#,
            r#"{"id":8,"delete_event":"uid-1"}"#,
        ] {
            assert_eq!(check(line), Ok(()), "{line}");
        }
    }

    #[test]
    fn a_kind_the_panel_does_not_send_is_refused_with_the_kind_named() {
        let error = check(r#"{"id":1,"hardware":true}"#).unwrap_err();
        assert!(error.contains("`hardware` is not a request the panel may send"), "{error}");
        for line in [
            r#"{"id":1,"text":"hi","language":"en","hardware":true}"#,
            r#"{"id":1,"cancel":"a1","shutdown":true}"#,
            r#"{"id":1,"text":"hi","language":"en","from":"2026-01-02"}"#,
            r#"{"id":1,"delete_event":"u","time":"09:00"}"#,
        ] {
            let error = check(line).unwrap_err();
            assert!(error.contains("not a request the panel may send"), "{line}: {error}");
        }
        assert!(check(r#"{"id":1,"text":"hi","language":"en","hardware":true}"#).unwrap_err().contains("`hardware`"));
    }

    #[test]
    fn two_allowed_kinds_or_none_are_refused() {
        for line in [r#"{"id":1,"confirm":"a","cancel":"a"}"#, r#"{"id":1}"#, "{}"] {
            let error = check(line).unwrap_err();
            assert!(error.contains("exactly one kind"), "{line}: {error}");
        }
    }

    #[test]
    fn what_is_not_a_json_object_is_refused() {
        for line in ["", "{not json", "[]", "\"text\"", "null", "7"] {
            let error = check(line).unwrap_err();
            assert!(error.contains("not a JSON object"), "{line:?}: {error}");
        }
    }
}
