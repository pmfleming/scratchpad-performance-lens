use std::path::Path;

pub(super) fn classify_path(path: &str) -> &'static str {
    let normalized = path.replace('\\', "/");
    let path = Path::new(&normalized);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let test_directory = path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some("test" | "tests" | "benches" | "testdata" | "fixtures")
        )
    });
    let test_file = file_name == "tests.rs"
        || file_name
            .strip_suffix(".rs")
            .is_some_and(|stem| stem.contains("_test") || stem.contains("test_"));

    if test_directory || test_file {
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
        let mut counts = (0, 0);
        let mut index = 0;

        while index < chars.len() {
            if self.skip_block_comment(&chars, &mut index) {
                continue;
            }
            match (chars[index], chars.get(index + 1)) {
                ('/', Some('/')) => break,
                ('/', Some('*')) => self.start_block_comment(&mut index),
                ('r', Some('"' | '#')) if skip_raw_string(&chars, &mut index) => {}
                ('"', _) => skip_quoted(&chars, &mut index, '"'),
                ('\'', _) => skip_quoted(&chars, &mut index, '\''),
                ('{', _) => {
                    counts.0 += 1;
                    index += 1;
                }
                ('}', _) => {
                    counts.1 += 1;
                    index += 1;
                }
                _ => index += 1,
            }
        }
        counts
    }

    fn skip_block_comment(&mut self, chars: &[char], index: &mut usize) -> bool {
        if self.block_comment_depth == 0 {
            return false;
        }
        match (chars[*index], chars.get(*index + 1)) {
            ('/', Some('*')) => self.start_block_comment(index),
            ('*', Some('/')) => {
                self.block_comment_depth -= 1;
                *index += 2;
            }
            _ => *index += 1,
        }
        true
    }

    fn start_block_comment(&mut self, index: &mut usize) {
        self.block_comment_depth += 1;
        *index += 2;
    }
}

fn skip_quoted(chars: &[char], index: &mut usize, quote: char) {
    *index += 1;
    while *index < chars.len() {
        match chars[*index] {
            '\\' => *index += 2,
            current if current == quote => {
                *index += 1;
                break;
            }
            _ => *index += 1,
        }
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
