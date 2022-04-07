use crate::pattern;
use crate::pattern::Mode;
use bstr::{BString, ByteSlice};

#[inline]
pub fn parse_line(mut line: &[u8]) -> Option<(BString, pattern::Mode)> {
    let mut mode = Mode::empty();
    if line.is_empty() {
        return None;
    };
    if line.first() == Some(&b'!') {
        mode |= Mode::NEGATIVE;
        line = &line[1..];
    } else if line.first() == Some(&b'\\') {
        let second = line.get(1);
        if second == Some(&b'!') || second == Some(&b'#') {
            line = &line[1..];
        }
    }
    if line.iter().all(|b| b.is_ascii_whitespace()) {
        return None;
    }
    let mut line = truncate_non_escaped_trailing_spaces(line);
    if line.last() == Some(&b'/') {
        mode |= Mode::MUST_BE_DIR;
        line.pop();
    }
    if !line.contains(&b'/') {
        mode |= Mode::NO_SUB_DIR;
    }
    if line.first() == Some(&b'*') && line[1..].find_byteset(br"*?[\").is_none() {
        mode |= Mode::ENDS_WITH;
    }
    Some((line, mode))
}

/// We always copy just because that's ultimately needed anyway, not because we always have to.
fn truncate_non_escaped_trailing_spaces(buf: &[u8]) -> BString {
    match buf.rfind_not_byteset(br"\ ") {
        Some(pos) if pos + 1 == buf.len() => buf.into(), // does not end in (escaped) whitespace
        None => buf.into(),
        Some(start_of_non_space) => {
            // This seems a bit strange but attempts to recreate the git implementation while
            // actually removing the escape characters before spaces. We leave other backslashes
            // for escapes to be handled by `glob/globset`.
            let mut res: BString = buf[..start_of_non_space + 1].into();

            let mut trailing_bytes = buf[start_of_non_space + 1..].iter();
            let mut bare_spaces = 0;
            while let Some(b) = trailing_bytes.next() {
                match b {
                    b' ' => {
                        bare_spaces += 1;
                    }
                    b'\\' => {
                        res.extend(std::iter::repeat(b' ').take(bare_spaces));
                        bare_spaces = 0;
                        // Skip what follows, like git does, but keep spaces if possible.
                        if trailing_bytes.next() == Some(&b' ') {
                            res.push(b' ');
                        }
                    }
                    _ => unreachable!("BUG: this must be either backslash or space"),
                }
            }
            res
        }
    }
}