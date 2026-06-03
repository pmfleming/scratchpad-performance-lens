use regex::Regex;

pub(super) fn classify_path(path: &str) -> &'static str {
    let normalized = path.replace('\\', "/");
    let test_re = Regex::new(
        r"(^|/)(tests?|benches|testdata|fixtures)(/|$)|(^|/)tests\.rs$|(_test|_tests|test_).*\.rs$",
    )
    .unwrap();
    if test_re.is_match(&normalized) {
        "test"
    } else if normalized.starts_with("src/") || normalized == "build.rs" {
        "application"
    } else {
        "other"
    }
}

pub(super) fn rust_test_line_mask(source: &str) -> Vec<bool> {
    let lines: Vec<_> = source.lines().collect();
    let mut mask = vec![false; lines.len()];
    let mut pending_cfg_test = false;
    let mut stack: Vec<bool> = Vec::new();
    let mut lexer = BraceLexer::default();
    for (index, line) in lines.iter().enumerate() {
        let stripped = line.trim();
        let inherited = *stack.last().unwrap_or(&false);
        let line_is_test = inherited || pending_cfg_test;
        if stripped.contains("#[cfg(test)]") || stripped.contains("cfg_attr(test") {
            pending_cfg_test = true;
            mask[index] = true;
        } else {
            mask[index] = line_is_test;
        }
        let (opens, closes) = lexer.count_braces(line);
        for _ in 0..opens {
            stack.push(inherited || pending_cfg_test);
        }
        if opens > 0 {
            pending_cfg_test = false;
        }
        for _ in 0..closes {
            stack.pop();
        }
    }
    mask
}

#[derive(Default)]
struct BraceLexer {
    block_comment_depth: usize,
}

impl BraceLexer {
    fn count_braces(&mut self, line: &str) -> (usize, usize) {
        let chars: Vec<char> = line.chars().collect();
        let mut opens = 0;
        let mut closes = 0;
        let mut index = 0;
        let mut in_string = false;
        let mut in_char = false;
        let mut escaped = false;

        while index < chars.len() {
            let current = chars[index];
            let next = chars.get(index + 1).copied();

            if self.block_comment_depth > 0 {
                match (current, next) {
                    ('/', Some('*')) => {
                        self.block_comment_depth += 1;
                        index += 2;
                    }
                    ('*', Some('/')) => {
                        self.block_comment_depth -= 1;
                        index += 2;
                    }
                    _ => index += 1,
                }
                continue;
            }

            if in_string {
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == '"' {
                    in_string = false;
                }
                index += 1;
                continue;
            }

            if in_char {
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == '\'' {
                    in_char = false;
                }
                index += 1;
                continue;
            }

            match (current, next) {
                ('/', Some('/')) => break,
                ('/', Some('*')) => {
                    self.block_comment_depth += 1;
                    index += 2;
                }
                ('r', Some('"')) | ('r', Some('#')) if skip_raw_string(&chars, &mut index) => {}
                ('"', _) => {
                    in_string = true;
                    index += 1;
                }
                ('\'', _) => {
                    in_char = true;
                    index += 1;
                }
                ('{', _) => {
                    opens += 1;
                    index += 1;
                }
                ('}', _) => {
                    closes += 1;
                    index += 1;
                }
                _ => index += 1,
            }
        }

        (opens, closes)
    }
}

fn skip_raw_string(chars: &[char], index: &mut usize) -> bool {
    let start = *index;
    if chars.get(start) != Some(&'r') {
        return false;
    }
    let mut hashes = 0;
    let mut cursor = start + 1;
    while chars.get(cursor) == Some(&'#') {
        hashes += 1;
        cursor += 1;
    }
    if chars.get(cursor) != Some(&'"') {
        return false;
    }
    cursor += 1;
    while cursor < chars.len() {
        if chars[cursor] == '"'
            && (0..hashes).all(|offset| chars.get(cursor + 1 + offset) == Some(&'#'))
        {
            *index = cursor + 1 + hashes;
            return true;
        }
        cursor += 1;
    }
    *index = chars.len();
    true
}

#[cfg(test)]
mod tests {
    use super::{classify_path, rust_test_line_mask};

    #[test]
    fn classify_path_separates_application_and_tests() {
        assert_eq!(classify_path("src/lib.rs"), "application");
        assert_eq!(classify_path("tests/cli.rs"), "test");
        assert_eq!(classify_path("benches/search.rs"), "test");
        assert_eq!(classify_path("README.md"), "other");
    }

    #[test]
    fn rust_test_line_mask_tracks_cfg_test_modules() {
        let source = r#"
pub fn app() {}

#[cfg(test)]
mod tests {
    #[test]
    fn sample() {
        assert!(true);
    }
}
"#;
        let mask = rust_test_line_mask(source);
        assert!(!mask[1]);
        assert!(mask[3]);
        assert!(mask[5]);
        assert!(mask[8]);
    }

    #[test]
    fn rust_test_line_mask_ignores_braces_in_literals_and_comments() {
        let source = r##"
pub fn app() {
    let text = "{ not a block }";
    let raw = r#"{ still not a block }"#;
    // } not a close
    /* { not an open either } */
}

#[cfg(test)]
mod tests {
    fn helper() {
        let ch = '}';
    }
}

pub fn after() {}
"##;
        let mask = rust_test_line_mask(source);
        assert!(!mask[1]);
        assert!(!mask[7]);
        assert!(mask[9]);
        assert!(mask[12]);
        assert!(!mask[15]);
    }
}
