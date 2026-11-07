//! The command templates: exact strings for good pieces, a refusal for every
//! hostile one, and the parsers of what the host prints back.

use cfdcraft_remote::{
    ContainerState, RemoteFile, TimeDir,
    commands::{
        CommandError, MAX_TAIL, check_text, df_command, download_command, files_command,
        inspect_args, parse_df, parse_du_lines, parse_inspect, parse_wc_lines, ps_label_args,
        read_text_command, rm_args, stop_args, tail_command, time_dirs_command, write_text_command,
    },
};

const WORK: &str = "/data/cfdcraft/work";
const ID: &str = "cfdcraft-demo-1-2";
const RUN: &str = "/data/cfdcraft/work/cfdcraft-demo-1-2";

/// The start of every command that reads or writes one file of a run: the real path of the file,
/// and only a path inside the run folder goes on.
fn guard(rel: &str) -> String {
    format!(
        "d=$(readlink -f '{RUN}') && p=$(readlink -f '{RUN}/{rel}') && case \"$p\" in \"$d\"/*)"
    )
}

#[test]
fn tail_is_a_one_based_offset_and_a_byte_cap() {
    assert_eq!(
        tail_command(WORK, ID, "log.simpleFoam", 0, 1048576).unwrap(),
        format!(
            "{} [ -f \"$p\" ] && tail -c +1 \"$p\" 2>/dev/null | head -c 1048576;; esac || true",
            guard("log.simpleFoam")
        )
    );
    assert_eq!(
        tail_command(WORK, ID, "log.simpleFoam", 4096, 100).unwrap(),
        format!(
            "{} [ -f \"$p\" ] && tail -c +4097 \"$p\" 2>/dev/null | head -c 100;; esac || true",
            guard("log.simpleFoam")
        )
    );
}

#[test]
fn the_cap_is_bounded() {
    let big = tail_command(WORK, ID, "log", 0, 5_000_000).unwrap();
    assert!(
        big.ends_with(&format!("head -c {MAX_TAIL};; esac || true")),
        "{big}"
    );
    assert_eq!(MAX_TAIL, 1_048_576);
    assert!(matches!(
        tail_command(WORK, ID, "log", 0, 0),
        Err(CommandError::Bounds { .. })
    ));
    assert!(matches!(
        tail_command(WORK, ID, "log", u64::MAX, 10),
        Err(CommandError::Bounds { .. })
    ));
}

#[test]
fn files_and_time_dirs_and_df_are_fixed_templates() {
    assert_eq!(
        files_command(WORK, ID, "postProcessing").unwrap(),
        format!(
            "cd '{RUN}' 2>/dev/null && find 'postProcessing' -type f -exec wc -c {{}} + 2>/dev/null || true"
        )
    );
    assert_eq!(
        time_dirs_command(WORK, ID).unwrap(),
        format!(
            "cd '{RUN}' 2>/dev/null && {{ du -sk [0-9]* 2>/dev/null; du -sk processor*/[0-9]* 2>/dev/null | awk '{{n=split($2,a,\"/\"); s[a[n]]+=$1}} END{{for(k in s) print s[k], k}}'; }} || true"
        )
    );
    assert_eq!(df_command(WORK).unwrap(), format!("df -Pk '{WORK}'"));
    assert_eq!(
        download_command(WORK, ID).unwrap(),
        format!("tar -C '{RUN}' --exclude='processor*' -cf - .")
    );
    assert_eq!(
        read_text_command(WORK, ID, "system/controlDict").unwrap(),
        format!(
            "{} [ -f \"$p\" ] && head -c 1048577 \"$p\";; *) exit 1;; esac",
            guard("system/controlDict")
        )
    );
}

#[test]
fn docker_arguments_are_separate_and_bounded() {
    assert_eq!(
        inspect_args(ID).unwrap(),
        [
            "inspect",
            "--format",
            "{{.State.Status}} {{.State.ExitCode}} {{.State.OOMKilled}}",
            ID
        ]
    );
    assert_eq!(stop_args(ID, 30).unwrap(), ["stop", "--time", "30", ID]);
    assert!(stop_args(ID, 300).is_ok());
    assert!(matches!(
        stop_args(ID, 301),
        Err(CommandError::Bounds { .. })
    ));
    assert_eq!(rm_args(ID).unwrap(), ["rm", "-f", ID]);
    let label = format!("cfdcraft.run={ID}");
    assert_eq!(
        ps_label_args(ID).unwrap(),
        [
            "ps".to_string(),
            "-a".into(),
            "--filter".into(),
            format!("label={label}"),
            "--filter".into(),
            format!("name=^{ID}$"),
            "--format".into(),
            "{{.Names}}".into(),
        ]
    );
    // Only a container this tool names may be touched.
    for f in [inspect_args, rm_args, ps_label_args] {
        assert!(f("other-demo-1").is_err());
    }
    assert!(stop_args("other-demo-1", 5).is_err());
}

#[test]
fn write_text_names_a_temporary_and_renames_it() {
    assert_eq!(
        write_text_command(WORK, ID, "system/controlDict").unwrap(),
        format!(
            "d=$(readlink -f '{RUN}') && q=$(readlink -f '{RUN}/system') && p=\"$q/controlDict\" && [ ! -L \"$p\" ] && case \"$q\" in \"$d\"|\"$d\"/*) rm -f \"$p.tmp\" && (set -C; cat > \"$p.tmp\") && mv \"$p.tmp\" \"$p\";; *) exit 1;; esac"
        )
    );
    assert!(check_text(&"a".repeat(1 << 20)).is_ok());
    assert!(matches!(
        check_text(&"a".repeat((1 << 20) + 1)),
        Err(CommandError::TextTooLarge { .. })
    ));
}

fn hostile() -> Vec<String> {
    let mut v: Vec<String> = [
        "a'b", "a b", "a;b", "a`b", "a$b", "a\nb", "..", "a/../b", "/abs", "a|b", "$(x)",
    ]
    .map(String::from)
    .to_vec();
    v.push("a".repeat(201));
    v
}

#[test]
fn hostile_pieces_never_reach_a_command() {
    // The 11 `pub fn` builders are tail, files, time_dirs, df, read_text,
    // write_text, download (they take the work folder), and inspect, stop, rm,
    // ps_label (docker arguments from a run id). Each piece kind is tried on every
    // builder that takes it: 10 take a run id, 4 a path, 7 the work folder.
    let table = hostile();
    assert_eq!(table.len(), 12);
    for bad in &table {
        let as_id: Vec<bool> = vec![
            tail_command(WORK, bad, "log", 0, 10).is_err(),
            files_command(WORK, bad, "log").is_err(),
            time_dirs_command(WORK, bad).is_err(),
            read_text_command(WORK, bad, "log").is_err(),
            write_text_command(WORK, bad, "log").is_err(),
            download_command(WORK, bad).is_err(),
            inspect_args(bad).is_err(),
            stop_args(bad, 5).is_err(),
            rm_args(bad).is_err(),
            ps_label_args(bad).is_err(),
        ];
        assert!(as_id.iter().all(|e| *e), "run id {bad:?}: {as_id:?}");

        let as_path: Vec<bool> = vec![
            tail_command(WORK, ID, bad, 0, 10).is_err(),
            files_command(WORK, ID, bad).is_err(),
            read_text_command(WORK, ID, bad).is_err(),
            write_text_command(WORK, ID, bad).is_err(),
        ];
        assert!(as_path.iter().all(|e| *e), "path {bad:?}: {as_path:?}");

        let work = format!("/{bad}");
        let as_work: Vec<bool> = vec![
            tail_command(&work, ID, "log", 0, 10).is_err(),
            files_command(&work, ID, "log").is_err(),
            time_dirs_command(&work, ID).is_err(),
            df_command(&work).is_err(),
            read_text_command(&work, ID, "log").is_err(),
            write_text_command(&work, ID, "log").is_err(),
            download_command(&work, ID).is_err(),
        ];
        assert!(as_work.iter().all(|e| *e), "work {bad:?}: {as_work:?}");
    }
    // A leading dash would be read as an option.
    assert!(files_command(WORK, ID, "-delete").is_err());
}

#[test]
fn the_work_dir_must_be_plain() {
    for bad in [
        "/data/cfdcraft/wo'rk",
        "data/cfdcraft/work",
        "/data/cfdcraft/work/",
        "/data//work",
        "/data/../work",
        "",
    ] {
        assert!(
            matches!(df_command(bad), Err(CommandError::Invalid { .. })),
            "{bad:?}"
        );
    }
    assert!(df_command("/data/cfdcraft/work").is_ok());
}

#[test]
fn parse_df_reads_the_available_column() {
    let good = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                /dev/sda1 100 40 60 40% /data\n";
    assert_eq!(parse_df(good), Some(60));
    // A single data line without a header, as the shell fake prints it.
    assert_eq!(parse_df("fake 100000000 1 123456 1% /\n"), Some(123456));
    // A long device name wraps the row onto two lines.
    assert_eq!(
        parse_df(
            "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/mapper/long\n 100 40 60 40% /data\n"
        ),
        None
    );
    assert_eq!(
        parse_df("Filesystem 1024-blocks Used Available Capacity Mounted on\n"),
        None
    );
    assert_eq!(parse_df(""), None);
    assert_eq!(parse_df("/dev/sda1 100 40 many 40% /data\n"), None);
}

#[test]
fn parse_wc_lines_skips_the_total_line_and_bad_lines() {
    let text = "  1500 ./postProcessing/probes/0/p\n    12 postProcessing/other\n  1512 total\n\
                garbage line\n abc ./x\n    5 ./has space/file\n";
    assert_eq!(
        parse_wc_lines(text),
        [
            RemoteFile {
                rel: "postProcessing/probes/0/p".into(),
                size: 1500
            },
            RemoteFile {
                rel: "postProcessing/other".into(),
                size: 12
            },
        ]
    );
    assert!(parse_wc_lines("").is_empty());
}

#[test]
fn parse_du_lines_keeps_numeric_folder_names_only() {
    let text = "4\t0\n8\t0.5\n12\t1e-05\n100\tpostProcessing\n3\t0.orig\nbad\t1\n7\tconstant\n";
    let names: Vec<_> = parse_du_lines(text).into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["0", "0.5", "1e-05"]);
    assert_eq!(
        parse_du_lines("8\t0.5\n"),
        [TimeDir {
            name: "0.5".into(),
            kb: 8
        }]
    );
}

#[test]
fn parse_inspect_reads_status_exit_and_oom() {
    assert_eq!(
        parse_inspect("running 0 false\n").unwrap(),
        ContainerState::Running
    );
    assert_eq!(
        parse_inspect("exited 137 true\n").unwrap(),
        ContainerState::Exited {
            code: 137,
            oom_killed: true
        }
    );
    assert_eq!(
        parse_inspect("exited 1 false").unwrap(),
        ContainerState::Exited {
            code: 1,
            oom_killed: false
        }
    );
    for bad in [
        "",
        "weird 0 false",
        "exited x false",
        "exited 0",
        "exited 0 maybe",
    ] {
        assert!(parse_inspect(bad).is_err(), "{bad:?}");
    }
}
