use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    terms: Vec<(Op, Vec<u64>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}

impl Range {
    pub fn parse(text: &str) -> Result<Range, String> {
        let text = text.trim();
        if text == "*" || text.is_empty() {
            return Ok(Range { terms: vec![] });
        }
        let mut terms = vec![];
        for bit in text.split(',') {
            let bit = bit.trim();
            let (op, rest) = [(">=", Op::Ge), ("<=", Op::Le), ("==", Op::Eq), (">", Op::Gt), ("<", Op::Lt)]
                .into_iter()
                .find_map(|(prefix, op)| bit.strip_prefix(prefix).map(|rest| (op, rest.trim())))
                .ok_or_else(|| format!("invalid version range \"{text}\""))?;
            let numbers = rest.split('.').map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().map_err(|_| format!("invalid version bound \"{rest}\""))?;
            if numbers.len() < 2 {
                return Err(format!("invalid version bound \"{rest}\": needs at least major.minor"));
            }
            terms.push((op, numbers));
        }
        Ok(Range { terms })
    }

    pub fn any(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn admits(&self, version: Option<&str>) -> bool {
        if self.any() {
            return true;
        }
        let Some(version) = version.and_then(numbers) else { return false };
        self.terms.iter().all(|(op, bound)| {
            let order = compare(&version, bound);
            match op {
                Op::Ge => order.is_ge(),
                Op::Gt => order.is_gt(),
                Op::Le => order.is_le(),
                Op::Lt => order.is_lt(),
                Op::Eq => order.is_eq(),
            }
        })
    }
}

pub fn numbers(text: &str) -> Option<Vec<u64>> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric() || (bytes[i - 1] == b'v' && (i == 1 || !bytes[i - 2].is_ascii_alphanumeric()));
        if boundary && bytes[i].is_ascii_digit() {
            let mut end = i;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || (bytes[end] == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit))) {
                end += 1;
            }
            let found = &text[i..end];
            if found.contains('.') {
                return found.split('.').map(|n| n.parse().ok()).collect();
            }
            i = end;
        } else {
            i += 1;
        }
    }
    None
}

pub fn compare(a: &[u64], b: &[u64]) -> Ordering {
    (0..a.len().max(b.len())).map(|i| a.get(i).copied().unwrap_or(0).cmp(&b.get(i).copied().unwrap_or(0))).find(|o| o.is_ne()).unwrap_or(Ordering::Equal)
}

pub fn compare_text(a: &str, b: &str) -> Ordering {
    compare(&numbers(a).unwrap_or_default(), &numbers(b).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_gate_known_and_unknown_versions() {
        let r = |s: &str| Range::parse(s).unwrap();
        assert!(r("*").admits(None));
        assert!(!r(">=2.1.263").admits(None));
        assert!(r(">=2.1.263").admits(Some("2.1.286 (Claude Code)")));
        assert!(!r("<2.1.263").admits(Some("2.1.286")));
        assert!(r(">=0.150, <0.200").admits(Some("codex-cli 0.162.0")));
        assert!(r("==1.0").admits(Some("v1.0.0")));
        assert!(Range::parse("~1.2").is_err());
        assert!(Range::parse(">=2").is_err());
    }

    #[test]
    fn versions_are_found_in_probe_output() {
        assert_eq!(numbers("2.1.296 (Claude Code)"), Some(vec![2, 1, 296]));
        assert_eq!(numbers("codex-cli 0.162.0"), Some(vec![0, 162, 0]));
        assert_eq!(numbers("mock-agent v1.4"), Some(vec![1, 4]));
        assert_eq!(numbers("no version"), None);
        assert_eq!(compare_text("2026.10.10.1", "2026.10.5.3"), Ordering::Greater);
    }
}
