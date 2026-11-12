//! S-expressions as KiCad writes them: lists, bare words and quoted strings.

#[derive(Debug, Clone, PartialEq)]
pub enum Sexp {
    /// A bare word or number.
    Atom(String),
    /// A quoted string.
    Text(String),
    List(Vec<Sexp>),
}

impl Sexp {
    /// The elements of a list; none for a word.
    pub fn items(&self) -> &[Sexp] {
        match self {
            Sexp::List(items) => items,
            _ => &[],
        }
    }

    /// The first word of a list: `segment` for `(segment ...)`.
    pub fn head(&self) -> Option<&str> {
        match self.items().first() {
            Some(Sexp::Atom(a)) => Some(a),
            _ => None,
        }
    }

    /// Word or string number `i` of a list (0 is the head).
    pub fn text(&self, i: usize) -> Option<&str> {
        match self.items().get(i) {
            Some(Sexp::Atom(a)) | Some(Sexp::Text(a)) => Some(a),
            _ => None,
        }
    }

    pub fn num(&self, i: usize) -> Option<f64> {
        self.text(i).and_then(|t| t.parse().ok())
    }

    /// The first sub-list with this head.
    pub fn find(&self, head: &str) -> Option<&Sexp> {
        self.items().iter().find(|s| s.head() == Some(head))
    }

    /// All sub-lists with this head.
    pub fn all<'a>(&'a self, head: &'a str) -> impl Iterator<Item = &'a Sexp> + 'a {
        self.items().iter().filter(move |s| s.head() == Some(head))
    }

    /// The words and strings after the head.
    pub fn words(&self) -> Vec<&str> {
        (1..self.items().len()).filter_map(|i| self.text(i)).collect()
    }
}

/// Parse one expression; what follows it is ignored.
pub fn parse(text: &str) -> Result<Sexp, String> {
    let bytes = text.as_bytes();
    let mut stack: Vec<Vec<Sexp>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => {
                stack.push(Vec::new());
                i += 1;
            }
            b')' => {
                let done = Sexp::List(stack.pop().ok_or("a closing bracket too many")?);
                match stack.last_mut() {
                    Some(parent) => parent.push(done),
                    None => return Ok(done),
                }
                i += 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'"' => {
                let mut out = Vec::new();
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                        out.push(match bytes[i] {
                            b'n' => b'\n',
                            b't' => b'\t',
                            c => c,
                        });
                    } else {
                        out.push(bytes[i]);
                    }
                    i += 1;
                }
                i += 1;
                let text = String::from_utf8(out).map_err(|e| e.to_string())?;
                stack.last_mut().ok_or("a string outside of any list")?.push(Sexp::Text(text));
            }
            _ => {
                let start = i;
                while i < bytes.len() && !matches!(bytes[i], b'(' | b')' | b' ' | b'\t' | b'\n' | b'\r' | b'"') {
                    i += 1;
                }
                stack
                    .last_mut()
                    .ok_or("a word outside of any list")?
                    .push(Sexp::Atom(text[start..i].to_string()));
            }
        }
    }
    Err("the file ends inside a list".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_words_and_strings() {
        let s = parse("(pad \"1\" smd (at -2.7 0 270) (net \"Net-(J2-\\\"A\\\")\"))").unwrap();
        assert_eq!(s.head(), Some("pad"));
        assert_eq!(s.words(), vec!["1", "smd"]);
        assert_eq!(s.find("at").unwrap().num(1), Some(-2.7));
        assert_eq!(s.find("at").unwrap().num(3), Some(270.0));
        assert_eq!(s.find("net").unwrap().text(1), Some("Net-(J2-\"A\")"));
        assert!(s.find("layer").is_none());
        assert!(parse("(a (b)").is_err());
    }
}
