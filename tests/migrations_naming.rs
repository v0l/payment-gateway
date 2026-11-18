use std::fs;

fn valid_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".sql") else {
        return false;
    };
    let Some((stamp, rest)) = stem.split_once('_') else {
        return false;
    };
    stamp.len() == 14
        && stamp.bytes().all(|b| b.is_ascii_digit())
        && !rest.is_empty()
        && rest
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[test]
fn migrations_after_0004_use_a_timestamp_name() {
    let legacy = ["0001", "0002", "0003", "0004"];
    for entry in fs::read_dir("migrations").unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if legacy.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        assert!(
            valid_name(&name),
            "{name} must match ^\\d{{14}}_[a-z0-9_]+\\.sql$"
        );
    }
}

#[test]
fn name_rule_rejects_small_numbers_and_bad_shapes() {
    assert!(valid_name("20261118093000_audit_log.sql"));
    assert!(!valid_name("0005_audit_log.sql"));
    assert!(!valid_name("20261118093000_Audit.sql"));
    assert!(!valid_name("20261118093000_.sql"));
    assert!(!valid_name("20261118093000_audit.txt"));
}
