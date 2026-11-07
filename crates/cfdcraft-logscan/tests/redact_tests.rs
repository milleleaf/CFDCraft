use cfdcraft_logscan::{MAX_LINE_CHARS, redact, tail_lines};

#[test]
fn redact_removes_paths_names_and_ids() {
    let text = "open /data/cfdcraft/work/cfdcraft-demo-123-456/log.simpleFoam failed\n\
                file \"/home/user/x\" missing\n\
                container cfdcraft-demo-123-456 stopped\n\
                id 0123456789abcdef0123456789abcdef01234567 gone\n\
                read /w/system/controlDict ok\n\
                ordinary words stay";
    let expected = "open <path> failed\n\
                    file \"<path>\" missing\n\
                    container <run> stopped\n\
                    id <id> gone\n\
                    read /w/system/controlDict ok\n\
                    ordinary words stay";
    assert_eq!(redact(text), expected);
}

#[test]
fn a_path_that_leaves_the_case_is_hidden() {
    assert_eq!(redact("see /w/../etc/passwd now"), "see <path> now");
    assert_eq!(redact("in \"/usr/lib/libc.so.6\""), "in \"<path>\"");
    assert_eq!(
        redact("file: /opt/x/dict.cfg!a/b at line 4."),
        "file: <path> at line 4."
    );
    assert_eq!(redact("cwd /w"), "cwd /w");
}

#[test]
fn numbers_and_ratios_are_untouched() {
    let line = "Initial residual = 1.40824257965e-05, Final residual = 5.73080745087e-11, 3/4 done";
    assert_eq!(redact(line), line);
}

#[test]
fn redact_caps_line_length_and_drops_control_characters() {
    let long = "z".repeat(500);
    assert_eq!(redact(&long).chars().count(), MAX_LINE_CHARS);
    assert_eq!(redact("a\u{1b}[31mb\rc\td"), "a[31mbc\td");
    assert_eq!(redact(""), "");
}

#[test]
fn tail_lines_returns_the_last_n_lines_and_never_splits_a_line() {
    let text = "one\ntwo\nthree\n";
    assert_eq!(tail_lines(text, 2), "two\nthree\n");
    assert_eq!(tail_lines(text, 1), "three\n");
    assert_eq!(tail_lines(text, 3), text);
    assert_eq!(tail_lines(text, 10), text);
    assert_eq!(tail_lines(text, 0), "");
    assert_eq!(tail_lines("one\ntwo", 1), "two");
    assert_eq!(tail_lines("", 3), "");
}

#[test]
fn a_sentence_is_not_cut_but_still_hides_paths() {
    let long = format!("{} reading /home/dt/secret from the host", "x".repeat(400));
    let kept = cfdcraft_logscan::redact_sentence(&long);
    assert!(kept.len() > cfdcraft_logscan::MAX_LINE_CHARS, "{kept}");
    assert!(!kept.contains("/home/dt"), "{kept}");
    assert!(kept.contains("<path>"), "{kept}");
}
