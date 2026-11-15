// Lenient JSON reader used by BeamNG files (.jbeam / .json / .pc):
// supports // and /* */ comments, missing commas, trailing commas.
#[derive(Clone, Debug)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, k: &str) -> Option<&J> {
        if let J::Obj(v) = self { v.iter().find(|(a, _)| a == k).map(|(_, b)| b) } else { None }
    }
    pub fn str(&self) -> Option<&str> { if let J::Str(s) = self { Some(s) } else { None } }
    pub fn num(&self) -> Option<f64> { if let J::Num(n) = self { Some(*n) } else { None } }
    pub fn arr(&self) -> Option<&Vec<J>> { if let J::Arr(a) = self { Some(a) } else { None } }
    pub fn obj(&self) -> Option<&Vec<(String, J)>> { if let J::Obj(a) = self { Some(a) } else { None } }
    pub fn get_str(&self, k: &str) -> Option<String> { self.get(k).and_then(|x| x.str()).map(|s| s.to_string()) }
}

struct P<'a> { s: &'a [u8], i: usize }

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' || c == b',' { self.i += 1; }
            else if c == 0xEF && self.s[self.i..].starts_with(&[0xEF, 0xBB, 0xBF]) { self.i += 3; }
            else if self.s[self.i..].starts_with(b"//") { while self.i < self.s.len() && self.s[self.i] != b'\n' { self.i += 1; } }
            else if self.s[self.i..].starts_with(b"/*") {
                self.i += 2;
                while self.i + 1 < self.s.len() && !(self.s[self.i] == b'*' && self.s[self.i + 1] == b'/') { self.i += 1; }
                self.i += 2;
            } else { break; }
        }
    }
    fn val(&mut self) -> Result<J, String> {
        self.ws();
        if self.i >= self.s.len() { return Err("eof".into()); }
        let c = self.s[self.i];
        match c {
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.i >= self.s.len() { return Err("eof in object".into()); }
                    if self.s[self.i] == b'}' { self.i += 1; return Ok(J::Obj(v)); }
                    let k = match self.val()? { J::Str(s) => s, J::Num(n) => n.to_string(), o => format!("{:?}", o) };
                    self.ws();
                    if self.i < self.s.len() && self.s[self.i] == b':' { self.i += 1; }
                    let x = self.val()?;
                    v.push((k, x));
                }
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.i >= self.s.len() { return Err("eof in array".into()); }
                    if self.s[self.i] == b']' { self.i += 1; return Ok(J::Arr(v)); }
                    v.push(self.val()?);
                }
            }
            b'"' => {
                let mut j = self.i + 1;
                let mut out: Vec<u8> = Vec::new();
                while j < self.s.len() && self.s[j] != b'"' {
                    if self.s[j] == b'\\' && j + 1 < self.s.len() {
                        j += 1;
                        match self.s[j] {
                            b'n' => out.push(b'\n'), b't' => out.push(b'\t'), b'r' => out.push(b'\r'),
                            b'u' if j + 4 < self.s.len() => {
                                let h = std::str::from_utf8(&self.s[j + 1..j + 5]).unwrap_or("0");
                                if let Some(ch) = u32::from_str_radix(h, 16).ok().and_then(char::from_u32) {
                                    let mut b = [0u8; 4];
                                    out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                                }
                                j += 4;
                            }
                            o => out.push(o),
                        }
                    } else { out.push(self.s[j]); }
                    j += 1;
                }
                self.i = j + 1;
                Ok(J::Str(String::from_utf8_lossy(&out).into_owned()))
            }
            _ => {
                let st = self.i;
                while self.i < self.s.len() {
                    let c = self.s[self.i];
                    if c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'+' || c == b'_' { self.i += 1; } else { break; }
                }
                if self.i == st { return Err(format!("bad char {:?} at {}", c as char, self.i)); }
                let w = std::str::from_utf8(&self.s[st..self.i]).unwrap_or("");
                match w {
                    "true" => Ok(J::Bool(true)),
                    "false" => Ok(J::Bool(false)),
                    "null" | "nil" => Ok(J::Null),
                    _ => match w.parse::<f64>() { Ok(n) => Ok(J::Num(n)), Err(_) => Ok(J::Str(w.to_string())) },
                }
            }
        }
    }
}

pub fn parse(text: &str) -> Result<J, String> {
    let mut p = P { s: text.as_bytes(), i: 0 };
    p.val()
}
