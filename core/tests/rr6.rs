//! RR6 pure tests: the person dates line (P-PPL-1). No server needed.
use jf_core::ui::person::life_line;

#[test]
fn born_and_died_gives_years_and_age() {
    let l = life_line(Some("1950-03-02T00:00:00.0000000Z"), Some("2020-07-01T00:00:00.0000000Z"));
    assert_eq!(l.as_deref(), Some("1950 \u{2013} 2020 \u{b7} aged 70"));
}

#[test]
fn age_is_one_less_before_the_birthday() {
    let l = life_line(Some("1950-09-02T00:00:00Z"), Some("2020-07-01T00:00:00Z"));
    assert_eq!(l.as_deref(), Some("1950 \u{2013} 2020 \u{b7} aged 69"));
}

#[test]
fn born_only_and_died_only_and_neither() {
    assert_eq!(life_line(Some("1981-01-05T00:00:00Z"), None).as_deref(), Some("Born 1981"));
    assert_eq!(life_line(None, Some("2001-05-05T00:00:00Z")).as_deref(), Some("Died 2001"));
    assert_eq!(life_line(None, None), None);
    assert_eq!(life_line(Some(""), Some("  ")), None);
}
