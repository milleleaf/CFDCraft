use std::path::{Path, PathBuf};

/// A Dockerfile larger than this is reported instead of read.
const MAX_DOCKERFILE_BYTES: u64 = 1 << 20;

/// What counts as a check of a downloaded file, when it sits in the same `RUN` as the download.
const CHECKS: &[&str] = &[
    "sha256sum -c",
    "sha256sum --check",
    "minisign -V",
    "gpg --verify",
];

/// One instruction with its backslash continuations joined; `line` is where it starts.
struct Instruction {
    line: usize,
    keyword: String,
    body: String,
}

/// Every `docker/*/Dockerfile` (not `docker/probes`) must start from a digest, check every
/// download, install without recommended packages, reset the entrypoint, put the solver on the
/// login-shell path and carry a source label. A repository without a `docker` folder passes.
pub fn check_dockerfiles(root: &Path) -> Result<(), String> {
    let docker = root.join("docker");
    if !docker.is_dir() {
        return Ok(());
    }
    let entries =
        std::fs::read_dir(&docker).map_err(|e| format!("cannot list {}: {e}", docker.display()))?;
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let dir = entry.map_err(|e| e.to_string())?.path();
        let skip = dir.file_name().is_some_and(|n| n == "probes");
        let file = dir.join("Dockerfile");
        if !skip && file.is_file() {
            files.push(file);
        }
    }
    files.sort();
    let mut problems = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let size = std::fs::metadata(&file)
            .map_err(|e| format!("cannot read {rel}: {e}"))?
            .len();
        if size > MAX_DOCKERFILE_BYTES {
            problems.push(format!(
                "DOCKERFILE-VIOLATION: {rel}:1: file is larger than {MAX_DOCKERFILE_BYTES} bytes and was not checked"
            ));
            continue;
        }
        let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {rel}: {e}"))?;
        problems.extend(check_text(&rel, &text));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn instruction(line: usize, joined: &str) -> Instruction {
    let joined = joined.trim();
    let (keyword, body) = joined
        .split_once(char::is_whitespace)
        .unwrap_or((joined, ""));
    Instruction {
        line,
        keyword: keyword.to_ascii_uppercase(),
        body: body.trim().to_string(),
    }
}

fn instructions(text: &str) -> Vec<Instruction> {
    let mut out = Vec::new();
    let mut current: Option<(usize, String)> = None;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with('#') || (line.is_empty() && current.is_none()) {
            continue;
        }
        let (start, mut joined) = current.take().unwrap_or((n + 1, String::new()));
        match line.strip_suffix('\\') {
            Some(part) => {
                joined.push_str(part.trim_end());
                joined.push(' ');
                current = Some((start, joined));
            }
            None => {
                joined.push_str(line);
                out.push(instruction(start, &joined));
            }
        }
    }
    if let Some((start, joined)) = current {
        out.push(instruction(start, &joined));
    }
    out
}

fn has_digest(image: &str) -> bool {
    match image.split_once("@sha256:") {
        Some((_, hex)) => hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
        None => false,
    }
}

/// The command segments of a shell line: split at `;`, `&`, `|`, parentheses, backticks.
fn segments(body: &str) -> Vec<Vec<&str>> {
    body.split([';', '&', '|', '(', ')', '`', '[', ']', ',', '"'])
        .map(|s| {
            let mut tokens: Vec<&str> = s.split_whitespace().collect();
            // Leading build flags, variable assignments and `sudo` do not name the command.
            while let Some(&first) = tokens.first() {
                if first.starts_with("--") || first.contains('=') || first == "sudo" {
                    tokens.remove(0);
                } else {
                    break;
                }
            }
            tokens
        })
        .filter(|t| !t.is_empty())
        .collect()
}

fn check_text(rel: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut add = |line: usize, what: &str| {
        found.push(format!("DOCKERFILE-VIOLATION: {rel}:{line}: {what}"));
    };
    let items = instructions(text);
    let mut stages: Vec<String> = Vec::new();
    let mut last_from = 0usize;
    let mut last_entrypoint: Option<(usize, bool)> = None;
    let mut profile_after_from = false;
    let mut label_after_from = false;
    for item in &items {
        match item.keyword.as_str() {
            "FROM" => {
                last_from = item.line;
                profile_after_from = false;
                label_after_from = false;
                let mut tokens = item
                    .body
                    .split_whitespace()
                    .filter(|t| !t.starts_with("--"));
                let image = tokens.next().unwrap_or("");
                let rest: Vec<&str> = tokens.collect();
                let is_stage = stages.iter().any(|s| *s == image.to_ascii_lowercase());
                let name = image.split('@').next().unwrap_or(image);
                if name.ends_with(":latest") {
                    add(item.line, "base image uses the latest tag");
                } else if image != "scratch" && !is_stage && !has_digest(image) {
                    add(
                        item.line,
                        "base image is not pinned by an @sha256 digest (a tag alone can move)",
                    );
                }
                if let [as_word, stage] = rest.as_slice()
                    && as_word.eq_ignore_ascii_case("as")
                {
                    stages.push(stage.to_ascii_lowercase());
                }
            }
            "ENTRYPOINT" => {
                let reset = item.body.split_whitespace().collect::<String>() == "[]";
                last_entrypoint = Some((item.line, reset));
            }
            "LABEL" => {
                if item.body.contains("org.opencontainers.image.source") {
                    label_after_from = true;
                }
            }
            "ADD" => {
                let remote = item
                    .body
                    .split_whitespace()
                    .any(|t| t.starts_with("http://") || t.starts_with("https://"));
                if remote && !item.body.contains("--checksum=") {
                    add(item.line, "ADD of a remote file without --checksum");
                }
            }
            "RUN" => {
                if item.body.contains("<<") {
                    add(
                        item.line,
                        "RUN with a here-document cannot be checked; use a plain RUN",
                    );
                    continue;
                }
                if item.body.contains("/etc/profile.d/") {
                    profile_after_from = true;
                }
                let flat = item.body.split_whitespace().collect::<Vec<_>>().join(" ");
                let checked = CHECKS.iter().any(|c| flat.contains(c));
                for seg in segments(&item.body) {
                    let command = seg[0];
                    if (command == "curl" || command == "wget") && !checked {
                        add(
                            item.line,
                            "download without sha256sum -c, minisign -V or gpg --verify in the same RUN",
                        );
                    }
                    if (command == "apt-get" || command == "apt")
                        && seg.contains(&"install")
                        && !seg.contains(&"--no-install-recommends")
                    {
                        add(item.line, "install without --no-install-recommends");
                    }
                }
            }
            _ => {}
        }
    }
    if items.is_empty() || last_from == 0 {
        add(1, "no FROM instruction");
        return found;
    }
    match last_entrypoint {
        Some((line, true)) if line > last_from => {}
        Some((line, false)) if line > last_from => {
            add(
                line,
                "ENTRYPOINT is set; the last stage must reset it with ENTRYPOINT []",
            );
        }
        _ => add(
            last_from,
            "the last stage does not reset the entrypoint with ENTRYPOINT []",
        ),
    }
    if !label_after_from {
        add(
            last_from,
            "the last stage has no LABEL org.opencontainers.image.source",
        );
    }
    if !profile_after_from {
        add(
            last_from,
            "the last stage writes nothing under /etc/profile.d/, so a login shell would not find the solver",
        );
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> String {
        format!("sha256:{}", "a".repeat(64))
    }

    const DOWNLOAD: &str = "RUN curl -fsSL -o /x.tgz https://example.org/x.tgz && echo \"00  /x.tgz\" | sha256sum -c -";
    const INSTALL: &str = "RUN apt-get update && apt-get install -y --no-install-recommends curl";

    fn file(run_lines: &[&str]) -> String {
        let mut text = format!("FROM base@{} AS build\n", digest());
        for l in run_lines {
            text.push_str(l);
            text.push('\n');
        }
        text.push_str("FROM build\n");
        text.push_str("LABEL org.opencontainers.image.source=\"https://example.org/src\"\n");
        text.push_str("RUN echo 'export PATH=/opt/x/bin:$PATH' > /etc/profile.d/x.sh\n");
        text.push_str("ENTRYPOINT []\n");
        text
    }

    fn good() -> String {
        file(&[INSTALL, "# download, then check in the same RUN", DOWNLOAD])
    }

    fn lines(text: &str) -> Vec<String> {
        check_text("docker/x/Dockerfile", text)
    }

    #[test]
    fn a_good_dockerfile_passes() {
        assert_eq!(lines(&good()), Vec::<String>::new());
    }

    #[test]
    fn a_tag_only_base_fails() {
        let bad = good().replace(&format!("base@{}", digest()), "ubuntu:26.04");
        let found = lines(&bad);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("docker/x/Dockerfile:1:"), "{found:?}");
        assert!(found[0].contains("digest"), "{found:?}");
        // The second FROM names the first stage and is not reported.
    }

    #[test]
    fn a_latest_tag_fails() {
        let bad = good().replace(&format!("base@{}", digest()), "ubuntu:latest");
        let found = lines(&bad);
        assert!(found.iter().any(|f| f.contains("latest tag")), "{found:?}");
    }

    #[test]
    fn a_download_without_a_check_fails() {
        let unchecked = "RUN curl -fsSL -o /x.tgz https://example.org/x.tgz";
        let found = lines(&file(&[INSTALL, unchecked]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("download without"), "{found:?}");
        let wget = "RUN wget -O /x.tgz https://example.org/x.tgz && cat /x.tgz";
        assert!(
            lines(&file(&[wget]))
                .iter()
                .any(|f| f.contains("download without"))
        );
    }

    #[test]
    fn installing_curl_is_not_a_download() {
        assert_eq!(lines(&file(&[INSTALL])), Vec::<String>::new());
    }

    #[test]
    fn a_missing_entrypoint_reset_fails() {
        let none = good().replace("ENTRYPOINT []\n", "");
        assert!(lines(&none).iter().any(|f| f.contains("ENTRYPOINT []")));
        let set = good().replace("ENTRYPOINT []", "ENTRYPOINT [\"code_saturne\"]");
        let found = lines(&set);
        assert!(
            found.iter().any(|f| f.contains("ENTRYPOINT is set")),
            "{found:?}"
        );
    }

    #[test]
    fn install_without_no_install_recommends_fails() {
        let found = lines(&file(&["RUN apt-get install -y curl"]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("--no-install-recommends"), "{found:?}");
    }

    #[test]
    fn a_missing_label_or_profile_script_fails() {
        let no_label = good().replace("LABEL org.opencontainers.image.source", "LABEL x");
        assert!(lines(&no_label).iter().any(|f| f.contains("LABEL")));
        let no_profile = good().replace("/etc/profile.d/x.sh", "/etc/x.sh");
        assert!(
            lines(&no_profile)
                .iter()
                .any(|f| f.contains("/etc/profile.d/"))
        );
    }

    #[test]
    fn a_folder_without_dockerfiles_passes() {
        let root = Path::new("/nonexistent-cfdcraft-root");
        assert_eq!(check_dockerfiles(root), Ok(()));
    }
}
