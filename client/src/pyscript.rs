// A small Python interpreter (subset) for in-game scripting: variables, numbers, strings, lists, dicts,
// if/elif/else, for, while, def/return, list comprehensions, f-strings, import (ignored), builtins.
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum V { None, Bool(bool), Int(i64), Float(f64), Str(Rc<str>), List(Rc<RefCell<Vec<V>>>), Dict(Rc<RefCell<Vec<(V, V)>>>), Func(Rc<Def>) }

#[derive(Debug)]
pub struct Def { name: String, params: Vec<(String, Option<Ex>)>, body: Vec<St> }

impl V {
    pub fn list(v: Vec<V>) -> V { V::List(Rc::new(RefCell::new(v))) }
    pub fn s(x: &str) -> V { V::Str(Rc::from(x)) }
    pub fn num(&self) -> Result<f64, String> { match self { V::Int(i) => Ok(*i as f64), V::Float(f) => Ok(*f), V::Bool(b) => Ok(*b as i32 as f64), _ => Err(format!("expected a number, got {}", self.repr())) } }
    pub fn truthy(&self) -> bool { match self { V::None => false, V::Bool(b) => *b, V::Int(i) => *i != 0, V::Float(f) => *f != 0.0, V::Str(s) => !s.is_empty(), V::List(l) => !l.borrow().is_empty(), V::Dict(d) => !d.borrow().is_empty(), V::Func(_) => true } }
    pub fn repr(&self) -> String {
        match self {
            V::None => "None".into(), V::Bool(b) => if *b { "True".into() } else { "False".into() },
            V::Int(i) => i.to_string(),
            V::Float(f) => { if f.fract() == 0.0 && f.abs() < 1e15 { format!("{:.1}", f) } else { format!("{}", (f * 1e6).round() / 1e6) } }
            V::Str(s) => format!("'{}'", s),
            V::List(l) => format!("[{}]", l.borrow().iter().map(|x| x.repr()).collect::<Vec<_>>().join(", ")),
            V::Dict(d) => format!("{{{}}}", d.borrow().iter().map(|(k, v)| format!("{}: {}", k.repr(), v.repr())).collect::<Vec<_>>().join(", ")),
            V::Func(f) => format!("<function {}>", f.name),
        }
    }
    pub fn text(&self) -> String { if let V::Str(s) = self { s.to_string() } else { self.repr() } }
    fn eq(&self, o: &V) -> bool {
        match (self, o) {
            (V::None, V::None) => true, (V::Str(a), V::Str(b)) => a == b,
            (V::List(a), V::List(b)) => { let (a, b) = (a.borrow(), b.borrow()); a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.eq(y)) }
            (V::Bool(a), V::Bool(b)) => a == b,
            _ => match (self.num(), o.num()) { (Ok(a), Ok(b)) => a == b, _ => false },
        }
    }
}

// ------------------------------------------------------------------ lexer
#[derive(Clone, Debug, PartialEq)]
enum T { Num(f64, bool), Str(String), FStr(String), Id(String), Op(&'static str), Nl, Indent, Dedent, End }

const OPS: [&str; 30] = ["**", "//", "==", "!=", "<=", ">=", "+=", "-=", "*=", "/=", "+", "-", "*", "/", "%", "<", ">", "=", "(", ")", "[", "]", "{", "}", ",", ":", ".", "%=", "//=", "->"];

fn lex(src: &str) -> Result<Vec<T>, String> {
    let mut toks = Vec::new();
    let mut indents = vec![0usize];
    let mut depth = 0i32;
    for (ln, line) in src.lines().enumerate() {
        let ch: Vec<char> = line.chars().collect();
        let mut i = 0;
        if depth == 0 {
            let mut col = 0;
            while i < ch.len() && (ch[i] == ' ' || ch[i] == '\t') { col += if ch[i] == '\t' { 4 } else { 1 }; i += 1; }
            if i >= ch.len() || ch[i] == '#' { continue; }
            if col > *indents.last().unwrap() { indents.push(col); toks.push(T::Indent); }
            while col < *indents.last().unwrap() { indents.pop(); toks.push(T::Dedent); }
            if col != *indents.last().unwrap() { return Err(format!("line {}: bad indentation", ln + 1)); }
        }
        while i < ch.len() {
            let c = ch[i];
            if c == ' ' || c == '\t' { i += 1; continue; }
            if c == '#' { break; }
            if c.is_ascii_digit() || (c == '.' && i + 1 < ch.len() && ch[i + 1].is_ascii_digit()) {
                let st = i; let mut fl = false;
                while i < ch.len() && (ch[i].is_ascii_digit() || ch[i] == '.' || ch[i] == '_' || ((ch[i] == 'e' || ch[i] == 'E') && i + 1 < ch.len() && (ch[i + 1].is_ascii_digit() || ch[i + 1] == '-'))) { if ch[i] == '.' || ch[i] == 'e' || ch[i] == 'E' { fl = true; } if (ch[i] == 'e' || ch[i] == 'E') && i + 1 < ch.len() && ch[i + 1] == '-' { i += 1; } i += 1; }
                let s: String = ch[st..i].iter().filter(|c| **c != '_').collect();
                toks.push(T::Num(s.parse().map_err(|_| format!("line {}: bad number {}", ln + 1, s))?, fl)); continue;
            }
            if c.is_alphabetic() || c == '_' {
                let st = i; while i < ch.len() && (ch[i].is_alphanumeric() || ch[i] == '_') { i += 1; }
                let w: String = ch[st..i].iter().collect();
                if (w == "f" || w == "F") && i < ch.len() && (ch[i] == '"' || ch[i] == '\'') {
                    let q = ch[i]; i += 1; let s0 = i; while i < ch.len() && ch[i] != q { i += 1; }
                    toks.push(T::FStr(ch[s0..i].iter().collect())); i += 1; continue;
                }
                toks.push(T::Id(w)); continue;
            }
            if c == '"' || c == '\'' {
                let q = c; i += 1; let mut s = String::new();
                while i < ch.len() && ch[i] != q { if ch[i] == '\\' && i + 1 < ch.len() { i += 1; s.push(match ch[i] { 'n' => '\n', 't' => '\t', o => o }); } else { s.push(ch[i]); } i += 1; }
                if i >= ch.len() { return Err(format!("line {}: unterminated string", ln + 1)); }
                i += 1; toks.push(T::Str(s)); continue;
            }
            let rest: String = ch[i..(i + 3).min(ch.len())].iter().collect();
            let mut found = None;
            for op in OPS.iter() { if rest.starts_with(op) && found.map(|f: &str| op.len() > f.len()).unwrap_or(true) { found = Some(*op); } }
            match found { Some(op) => { if op == "(" || op == "[" || op == "{" { depth += 1; } if op == ")" || op == "]" || op == "}" { depth -= 1; } toks.push(T::Op(op)); i += op.len(); } None => return Err(format!("line {}: unexpected character '{}'", ln + 1, c)) }
        }
        if depth == 0 { toks.push(T::Nl); }
    }
    while indents.len() > 1 { indents.pop(); toks.push(T::Dedent); }
    toks.push(T::End);
    Ok(toks)
}

// ------------------------------------------------------------------ AST + parser
#[derive(Clone, Debug)]
pub enum Ex {
    Lit(LitV), Name(String), Bin(&'static str, Box<Ex>, Box<Ex>), Un(&'static str, Box<Ex>), And(Box<Ex>, Box<Ex>), Or(Box<Ex>, Box<Ex>),
    Cmp(Box<Ex>, Vec<(&'static str, Ex)>), Call(Box<Ex>, Vec<Ex>, Vec<(String, Ex)>), Idx(Box<Ex>, Box<Ex>), Attr(Box<Ex>, String),
    List(Vec<Ex>), Dict(Vec<(Ex, Ex)>), Cond(Box<Ex>, Box<Ex>, Box<Ex>), Comp(Box<Ex>, String, Box<Ex>, Option<Box<Ex>>), FStr(String),
}
#[derive(Clone, Debug)]
pub enum LitV { None, Bool(bool), Int(i64), Float(f64), Str(String) }
#[derive(Clone, Debug)]
pub enum St {
    Expr(Ex), Assign(Ex, Ex), Aug(Ex, &'static str, Ex), If(Vec<(Ex, Vec<St>)>, Vec<St>), For(String, Ex, Vec<St>), While(Ex, Vec<St>),
    Def(Rc<Def>), Return(Option<Ex>), Break, Continue, Pass, Global(Vec<String>),
}
impl Clone for Def { fn clone(&self) -> Self { Def { name: self.name.clone(), params: self.params.clone(), body: self.body.clone() } } }

struct Parser { t: Vec<T>, p: usize }
type R<X> = Result<X, String>;

impl Parser {
    fn peek(&self) -> &T { &self.t[self.p] }
    fn next(&mut self) -> T { let t = self.t[self.p].clone(); if self.p < self.t.len() - 1 { self.p += 1; } t }
    fn is_op(&self, o: &str) -> bool { matches!(self.peek(), T::Op(x) if *x == o) }
    fn is_id(&self, w: &str) -> bool { matches!(self.peek(), T::Id(x) if x == w) }
    fn eat_op(&mut self, o: &str) -> bool { if self.is_op(o) { self.p += 1; true } else { false } }
    fn eat_id(&mut self, w: &str) -> bool { if self.is_id(w) { self.p += 1; true } else { false } }
    fn expect_op(&mut self, o: &str) -> R<()> { if self.eat_op(o) { Ok(()) } else { Err(format!("expected '{}' but found {:?}", o, self.peek())) } }
    fn block(&mut self) -> R<Vec<St>> {
        self.expect_op(":")?;
        if matches!(self.peek(), T::Nl) {
            self.next();
            if !matches!(self.peek(), T::Indent) { return Err("expected an indented block".into()); }
            self.next();
            let mut v = Vec::new();
            while !matches!(self.peek(), T::Dedent | T::End) { v.push(self.stmt()?); }
            if matches!(self.peek(), T::Dedent) { self.next(); }
            Ok(v)
        } else { let s = self.stmt()?; Ok(vec![s]) }
    }
    fn stmt(&mut self) -> R<St> {
        if let T::Id(w) = self.peek().clone() {
            match w.as_str() {
                "if" => { self.next(); let mut arms = Vec::new(); let c = self.expr()?; let b = self.block()?; arms.push((c, b)); let mut els = Vec::new();
                    loop { if self.eat_id("elif") { let c = self.expr()?; let b = self.block()?; arms.push((c, b)); } else if self.eat_id("else") { els = self.block()?; break; } else { break; } }
                    return Ok(St::If(arms, els)); }
                "for" => { self.next(); let v = match self.next() { T::Id(n) => n, o => return Err(format!("bad for target {:?}", o)) }; if !self.eat_id("in") { return Err("expected 'in'".into()); } let it = self.expr()?; let b = self.block()?; return Ok(St::For(v, it, b)); }
                "while" => { self.next(); let c = self.expr()?; let b = self.block()?; return Ok(St::While(c, b)); }
                "def" => { self.next(); let name = match self.next() { T::Id(n) => n, o => return Err(format!("bad def name {:?}", o)) }; self.expect_op("(")?; let mut params = Vec::new();
                    while !self.is_op(")") { let pn = match self.next() { T::Id(n) => n, o => return Err(format!("bad parameter {:?}", o)) }; let d = if self.eat_op("=") { Some(self.expr()?) } else { None }; params.push((pn, d)); if !self.eat_op(",") { break; } }
                    self.expect_op(")")?; if self.eat_op("->") { self.expr()?; } let body = self.block()?; return Ok(St::Def(Rc::new(Def { name, params, body }))); }
                "return" => { self.next(); let e = if matches!(self.peek(), T::Nl | T::End) { None } else { Some(self.expr_list()?) }; self.nl(); return Ok(St::Return(e)); }
                "break" => { self.next(); self.nl(); return Ok(St::Break); }
                "continue" => { self.next(); self.nl(); return Ok(St::Continue); }
                "pass" => { self.next(); self.nl(); return Ok(St::Pass); }
                "import" | "from" => { while !matches!(self.peek(), T::Nl | T::End) { self.next(); } self.nl(); return Ok(St::Pass); }
                "global" => { self.next(); let mut v = Vec::new(); loop { if let T::Id(n) = self.next() { v.push(n); } if !self.eat_op(",") { break; } } self.nl(); return Ok(St::Global(v)); }
                _ => {}
            }
        }
        let e = self.expr_list()?;
        let st = if self.eat_op("=") { let r = self.expr_list()?; St::Assign(e, r) }
            else if let Some(op) = ["+=", "-=", "*=", "/=", "%="].iter().find(|o| self.is_op(o)) { let op = *op; self.next(); let r = self.expr()?; St::Aug(e, &op[..1], r) }
            else { St::Expr(e) };
        self.nl(); Ok(st)
    }
    fn nl(&mut self) { if matches!(self.peek(), T::Nl) { self.next(); } }
    fn expr_list(&mut self) -> R<Ex> {
        let first = self.expr()?;
        if self.is_op(",") { let mut v = vec![first]; while self.eat_op(",") { if matches!(self.peek(), T::Nl | T::End) || self.is_op("=") { break; } v.push(self.expr()?); } return Ok(Ex::List(v)); }
        Ok(first)
    }
    fn expr(&mut self) -> R<Ex> {
        let e = self.or()?;
        if self.is_id("if") { self.next(); let c = self.or()?; if !self.eat_id("else") { return Err("expected 'else'".into()); } let o = self.expr()?; return Ok(Ex::Cond(Box::new(c), Box::new(e), Box::new(o))); }
        Ok(e)
    }
    fn or(&mut self) -> R<Ex> { let mut l = self.and()?; while self.eat_id("or") { let r = self.and()?; l = Ex::Or(Box::new(l), Box::new(r)); } Ok(l) }
    fn and(&mut self) -> R<Ex> { let mut l = self.not()?; while self.eat_id("and") { let r = self.not()?; l = Ex::And(Box::new(l), Box::new(r)); } Ok(l) }
    fn not(&mut self) -> R<Ex> { if self.eat_id("not") { let e = self.not()?; return Ok(Ex::Un("not", Box::new(e))); } self.cmp() }
    fn cmp(&mut self) -> R<Ex> {
        let l = self.arith()?; let mut rest = Vec::new();
        loop {
            let op: &'static str = if self.eat_op("==") { "==" } else if self.eat_op("!=") { "!=" } else if self.eat_op("<=") { "<=" } else if self.eat_op(">=") { ">=" } else if self.eat_op("<") { "<" } else if self.eat_op(">") { ">" }
                else if self.is_id("in") { self.next(); "in" } else if self.is_id("not") && matches!(self.t.get(self.p + 1), Some(T::Id(w)) if w == "in") { self.next(); self.next(); "not in" } else if self.eat_id("is") { if self.eat_id("not") { "is not" } else { "is" } } else { break };
            rest.push((op, self.arith()?));
        }
        Ok(if rest.is_empty() { l } else { Ex::Cmp(Box::new(l), rest) })
    }
    fn arith(&mut self) -> R<Ex> { let mut l = self.term()?; loop { let op = if self.eat_op("+") { "+" } else if self.eat_op("-") { "-" } else { break }; let r = self.term()?; l = Ex::Bin(op, Box::new(l), Box::new(r)); } Ok(l) }
    fn term(&mut self) -> R<Ex> { let mut l = self.unary()?; loop { let op = if self.eat_op("*") { "*" } else if self.eat_op("//") { "//" } else if self.eat_op("/") { "/" } else if self.eat_op("%") { "%" } else { break }; let r = self.unary()?; l = Ex::Bin(op, Box::new(l), Box::new(r)); } Ok(l) }
    fn unary(&mut self) -> R<Ex> { if self.eat_op("-") { let e = self.unary()?; return Ok(Ex::Un("-", Box::new(e))); } if self.eat_op("+") { return self.unary(); } self.power() }
    fn power(&mut self) -> R<Ex> { let b = self.postfix()?; if self.eat_op("**") { let e = self.unary()?; return Ok(Ex::Bin("**", Box::new(b), Box::new(e))); } Ok(b) }
    fn postfix(&mut self) -> R<Ex> {
        let mut e = self.atom()?;
        loop {
            if self.eat_op("(") {
                let mut args = Vec::new(); let mut kw = Vec::new();
                while !self.is_op(")") {
                    if let (T::Id(n), Some(T::Op("="))) = (self.peek().clone(), self.t.get(self.p + 1)) { self.next(); self.next(); kw.push((n, self.expr()?)); } else { args.push(self.expr()?); }
                    if !self.eat_op(",") { break; }
                }
                self.expect_op(")")?; e = Ex::Call(Box::new(e), args, kw);
            } else if self.eat_op("[") { let i = self.expr()?; self.expect_op("]")?; e = Ex::Idx(Box::new(e), Box::new(i)); }
            else if self.eat_op(".") { match self.next() { T::Id(n) => e = Ex::Attr(Box::new(e), n), o => return Err(format!("bad attribute {:?}", o)) } }
            else { break; }
        }
        Ok(e)
    }
    fn atom(&mut self) -> R<Ex> {
        match self.next() {
            T::Num(n, fl) => Ok(Ex::Lit(if fl { LitV::Float(n) } else { LitV::Int(n as i64) })),
            T::Str(s) => { let mut s = s; while let T::Str(t) = self.peek().clone() { self.next(); s.push_str(&t); } Ok(Ex::Lit(LitV::Str(s))) }
            T::FStr(s) => Ok(Ex::FStr(s)),
            T::Id(w) => Ok(match w.as_str() { "True" => Ex::Lit(LitV::Bool(true)), "False" => Ex::Lit(LitV::Bool(false)), "None" => Ex::Lit(LitV::None), _ => Ex::Name(w) }),
            T::Op("(") => { if self.eat_op(")") { return Ok(Ex::List(vec![])); } let e = self.expr_list()?; self.expect_op(")")?; Ok(e) }
            T::Op("[") => {
                if self.eat_op("]") { return Ok(Ex::List(vec![])); }
                let first = self.expr()?;
                if self.eat_id("for") {
                    let v = match self.next() { T::Id(n) => n, o => return Err(format!("bad comprehension target {:?}", o)) };
                    if !self.eat_id("in") { return Err("expected 'in'".into()); }
                    let it = self.or()?; let cond = if self.eat_id("if") { Some(Box::new(self.or()?)) } else { None };
                    self.expect_op("]")?; return Ok(Ex::Comp(Box::new(first), v, Box::new(it), cond));
                }
                let mut v = vec![first]; while self.eat_op(",") { if self.is_op("]") { break; } v.push(self.expr()?); }
                self.expect_op("]")?; Ok(Ex::List(v))
            }
            T::Op("{") => { let mut v = Vec::new(); while !self.is_op("}") { let k = self.expr()?; self.expect_op(":")?; let val = self.expr()?; v.push((k, val)); if !self.eat_op(",") { break; } } self.expect_op("}")?; Ok(Ex::Dict(v)) }
            o => Err(format!("unexpected token {:?}", o)),
        }
    }
}

// ------------------------------------------------------------------ interpreter
pub trait Host { fn call(&mut self, name: &str, args: &[V]) -> Option<Result<V, String>>; }

enum Flow { Next, Break, Continue, Return(V) }

pub struct Interp { globals: HashMap<String, V>, frames: Vec<HashMap<String, V>>, pub out: Vec<String>, steps: u64, global_decl: Vec<Vec<String>> }

const BUILTINS: [&str; 22] = ["print", "range", "len", "abs", "min", "max", "int", "float", "str", "round", "sqrt", "sin", "cos", "tan", "atan2", "radians", "degrees", "random", "randint", "sum", "list", "enumerate"];

impl Interp {
    pub fn new() -> Interp {
        let mut g = HashMap::new();
        g.insert("pi".into(), V::Float(std::f64::consts::PI)); g.insert("math".into(), V::None);
        Interp { globals: g, frames: vec![], out: vec![], steps: 0, global_decl: vec![] }
    }
    pub fn run(&mut self, src: &str, host: &mut dyn Host) -> Result<(), String> {
        let toks = lex(src)?;
        let mut p = Parser { t: toks, p: 0 };
        let mut prog = Vec::new();
        while !matches!(p.peek(), T::End) { if matches!(p.peek(), T::Nl) { p.next(); continue; } prog.push(p.stmt()?); }
        self.steps = 0;
        match self.exec_block(&prog, host)? { _ => Ok(()) }
    }
    fn get(&self, n: &str) -> Option<V> {
        if let Some(f) = self.frames.last() { if !self.global_decl.last().map(|g| g.iter().any(|x| x == n)).unwrap_or(false) { if let Some(v) = f.get(n) { return Some(v.clone()); } } }
        self.globals.get(n).cloned()
    }
    fn set(&mut self, n: &str, v: V) {
        if let Some(f) = self.frames.last_mut() { if !self.global_decl.last().map(|g| g.iter().any(|x| x == n)).unwrap_or(false) { f.insert(n.into(), v); return; } }
        self.globals.insert(n.into(), v);
    }
    fn exec_block(&mut self, b: &[St], h: &mut dyn Host) -> R<Flow> {
        for s in b { match self.exec(s, h)? { Flow::Next => {} f => return Ok(f) } }
        Ok(Flow::Next)
    }
    fn assign(&mut self, target: &Ex, v: V, h: &mut dyn Host) -> R<()> {
        match target {
            Ex::Name(n) => { self.set(n, v); Ok(()) }
            Ex::List(ts) => { if let V::List(l) = &v { let l = l.borrow().clone(); if l.len() != ts.len() { return Err("cannot unpack: wrong number of values".into()); } for (t, x) in ts.iter().zip(l) { self.assign(t, x, h)?; } Ok(()) } else { Err("cannot unpack a non-list".into()) } }
            Ex::Idx(c, i) => { let cv = self.eval(c, h)?; let iv = self.eval(i, h)?;
                match cv { V::List(l) => { let n = l.borrow().len() as i64; let mut k = iv.num()? as i64; if k < 0 { k += n; } if k < 0 || k >= n { return Err("list index out of range".into()); } l.borrow_mut()[k as usize] = v; Ok(()) }
                    V::Dict(d) => { let mut d = d.borrow_mut(); if let Some(e) = d.iter_mut().find(|(k, _)| k.eq(&iv)) { e.1 = v; } else { d.push((iv, v)); } Ok(()) }
                    _ => Err("item assignment not supported".into()) } }
            _ => Err("cannot assign to this expression".into()),
        }
    }
    fn tick(&mut self) -> R<()> { self.steps += 1; if self.steps > 3_000_000 { Err("script stopped: too many steps (infinite loop?)".into()) } else { Ok(()) } }
    fn exec(&mut self, s: &St, h: &mut dyn Host) -> R<Flow> {
        self.tick()?;
        match s {
            St::Expr(e) => { self.eval(e, h)?; }
            St::Assign(t, e) => { let v = self.eval(e, h)?; self.assign(t, v, h)?; }
            St::Aug(t, op, e) => { let cur = self.eval(t, h)?; let r = self.eval(e, h)?; let v = self.binop(op, &cur, &r)?; self.assign(t, v, h)?; }
            St::If(arms, els) => { for (c, b) in arms { if self.eval(c, h)?.truthy() { return self.exec_block(b, h); } } return self.exec_block(els, h); }
            St::While(c, b) => { while self.eval(c, h)?.truthy() { self.tick()?; match self.exec_block(b, h)? { Flow::Break => break, Flow::Return(v) => return Ok(Flow::Return(v)), _ => {} } } }
            St::For(var, it, b) => { let itv = self.eval(it, h)?; let items = self.iter(&itv)?; for x in items { self.set(var, x); self.tick()?; match self.exec_block(b, h)? { Flow::Break => break, Flow::Return(v) => return Ok(Flow::Return(v)), _ => {} } } }
            St::Def(d) => { self.set(&d.name.clone(), V::Func(d.clone())); }
            St::Return(e) => { let v = match e { Some(e) => self.eval(e, h)?, None => V::None }; return Ok(Flow::Return(v)); }
            St::Break => return Ok(Flow::Break), St::Continue => return Ok(Flow::Continue), St::Pass => {}
            St::Global(names) => { if let Some(g) = self.global_decl.last_mut() { g.extend(names.clone()); } }
        }
        Ok(Flow::Next)
    }
    fn iter(&self, v: &V) -> R<Vec<V>> {
        match v { V::List(l) => Ok(l.borrow().clone()), V::Str(s) => Ok(s.chars().map(|c| V::s(&c.to_string())).collect()), V::Dict(d) => Ok(d.borrow().iter().map(|(k, _)| k.clone()).collect()), _ => Err(format!("{} is not iterable", v.repr())) }
    }
    fn binop(&self, op: &str, a: &V, b: &V) -> R<V> {
        match (op, a, b) {
            ("+", V::Str(x), V::Str(y)) => Ok(V::s(&format!("{}{}", x, y))),
            ("+", V::List(x), V::List(y)) => { let mut v = x.borrow().clone(); v.extend(y.borrow().iter().cloned()); Ok(V::list(v)) }
            ("*", V::Str(x), n) | ("*", n, V::Str(x)) => Ok(V::s(&x.repeat(n.num()?.max(0.0) as usize))),
            ("*", V::List(x), n) | ("*", n, V::List(x)) => { let mut v = Vec::new(); for _ in 0..n.num()?.max(0.0) as usize { v.extend(x.borrow().iter().cloned()); } Ok(V::list(v)) }
            ("%", V::Str(f), args) => { let vals: Vec<V> = if let V::List(l) = args { l.borrow().clone() } else { vec![args.clone()] }; let mut out = String::new(); let mut it = vals.iter(); let ch: Vec<char> = f.chars().collect(); let mut i = 0;
                while i < ch.len() { if ch[i] == '%' && i + 1 < ch.len() { let mut j = i + 1; let mut spec = String::new(); while j < ch.len() && (ch[j].is_ascii_digit() || ch[j] == '.' || ch[j] == '-') { spec.push(ch[j]); j += 1; } if j >= ch.len() { break; }
                    match ch[j] { '%' => out.push('%'), 'd' | 'i' => out.push_str(&format!("{}", it.next().map(|v| v.num().unwrap_or(0.0) as i64).unwrap_or(0))), 's' => out.push_str(&it.next().map(|v| v.text()).unwrap_or_default()),
                        'f' => { let p: usize = spec.split('.').nth(1).and_then(|x| x.parse().ok()).unwrap_or(6); out.push_str(&format!("{:.*}", p, it.next().map(|v| v.num().unwrap_or(0.0)).unwrap_or(0.0))); } _ => {} } i = j + 1; } else { out.push(ch[i]); i += 1; } }
                Ok(V::s(&out)) }
            _ => {
                let (x, y) = (a.num()?, b.num()?);
                let both_int = matches!((a, b), (V::Int(_) | V::Bool(_), V::Int(_) | V::Bool(_)));
                Ok(match op {
                    "+" => if both_int { V::Int(x as i64 + y as i64) } else { V::Float(x + y) },
                    "-" => if both_int { V::Int(x as i64 - y as i64) } else { V::Float(x - y) },
                    "*" => if both_int { V::Int((x as i64).wrapping_mul(y as i64)) } else { V::Float(x * y) },
                    "/" => { if y == 0.0 { return Err("division by zero".into()); } V::Float(x / y) }
                    "//" => { if y == 0.0 { return Err("division by zero".into()); } let r = (x / y).floor(); if both_int { V::Int(r as i64) } else { V::Float(r) } }
                    "%" => { if y == 0.0 { return Err("modulo by zero".into()); } let r = x - y * (x / y).floor(); if both_int { V::Int(r as i64) } else { V::Float(r) } }
                    "**" => if both_int && y >= 0.0 { V::Int((x as i64).pow(y as u32)) } else { V::Float(x.powf(y)) },
                    _ => return Err(format!("unsupported operator {}", op)),
                })
            }
        }
    }
    fn eval(&mut self, e: &Ex, h: &mut dyn Host) -> R<V> {
        self.tick()?;
        match e {
            Ex::Lit(l) => Ok(match l { LitV::None => V::None, LitV::Bool(b) => V::Bool(*b), LitV::Int(i) => V::Int(*i), LitV::Float(f) => V::Float(*f), LitV::Str(s) => V::s(s) }),
            Ex::Name(n) => { if let Some(v) = self.get(n) { return Ok(v); } if BUILTINS.contains(&n.as_str()) { return Ok(V::s(&format!("<builtin {}>", n))); } Err(format!("name '{}' is not defined", n)) }
            Ex::Bin(op, a, b) => { let x = self.eval(a, h)?; let y = self.eval(b, h)?; self.binop(op, &x, &y) }
            Ex::Un(op, a) => { let x = self.eval(a, h)?; match *op { "not" => Ok(V::Bool(!x.truthy())), _ => match x { V::Int(i) => Ok(V::Int(-i)), o => Ok(V::Float(-o.num()?)) } } }
            Ex::And(a, b) => { let x = self.eval(a, h)?; if !x.truthy() { Ok(x) } else { self.eval(b, h) } }
            Ex::Or(a, b) => { let x = self.eval(a, h)?; if x.truthy() { Ok(x) } else { self.eval(b, h) } }
            Ex::Cond(c, a, b) => { if self.eval(c, h)?.truthy() { self.eval(a, h) } else { self.eval(b, h) } }
            Ex::Cmp(first, rest) => {
                let mut l = self.eval(first, h)?;
                for (op, re) in rest {
                    let r = self.eval(re, h)?;
                    let ok = match *op {
                        "==" | "is" => l.eq(&r), "!=" | "is not" => !l.eq(&r),
                        "in" | "not in" => { let inn = match &r { V::List(x) => x.borrow().iter().any(|y| y.eq(&l)), V::Str(s) => s.contains(&l.text()), V::Dict(d) => d.borrow().iter().any(|(k, _)| k.eq(&l)), _ => false }; if *op == "in" { inn } else { !inn } }
                        _ => { let (a, b) = (l.num()?, r.num()?); match *op { "<" => a < b, ">" => a > b, "<=" => a <= b, _ => a >= b } }
                    };
                    if !ok { return Ok(V::Bool(false)); }
                    l = r;
                }
                Ok(V::Bool(true))
            }
            Ex::List(v) => { let mut o = Vec::new(); for x in v { o.push(self.eval(x, h)?); } Ok(V::list(o)) }
            Ex::Dict(v) => { let mut o = Vec::new(); for (k, x) in v { o.push((self.eval(k, h)?, self.eval(x, h)?)); } Ok(V::Dict(Rc::new(RefCell::new(o)))) }
            Ex::Comp(body, var, it, cond) => { let itv = self.eval(it, h)?; let items = self.iter(&itv)?; let mut out = Vec::new(); for x in items { self.set(var, x); if let Some(c) = cond { if !self.eval(c, h)?.truthy() { continue; } } out.push(self.eval(body, h)?); } Ok(V::list(out)) }
            Ex::Idx(c, i) => { let cv = self.eval(c, h)?; let iv = self.eval(i, h)?;
                match cv { V::List(l) => { let l = l.borrow(); let n = l.len() as i64; let mut k = iv.num()? as i64; if k < 0 { k += n; } l.get(k as usize).cloned().ok_or_else(|| "list index out of range".to_string()) }
                    V::Str(s) => { let ch: Vec<char> = s.chars().collect(); let n = ch.len() as i64; let mut k = iv.num()? as i64; if k < 0 { k += n; } ch.get(k as usize).map(|c| V::s(&c.to_string())).ok_or_else(|| "string index out of range".to_string()) }
                    V::Dict(d) => d.borrow().iter().find(|(k, _)| k.eq(&iv)).map(|(_, v)| v.clone()).ok_or_else(|| format!("key {} not found", iv.repr())),
                    _ => Err("object is not subscriptable".into()) } }
            Ex::FStr(s) => { let mut out = String::new(); let ch: Vec<char> = s.chars().collect(); let mut i = 0;
                while i < ch.len() { if ch[i] == '{' { let mut j = i + 1; let mut depth = 1; while j < ch.len() { if ch[j] == '{' { depth += 1; } if ch[j] == '}' { depth -= 1; if depth == 0 { break; } } j += 1; }
                    let inner: String = ch[i + 1..j].iter().collect(); let (expr_s, spec) = match inner.rfind(':') { Some(k) if !inner[k..].contains(')') => (inner[..k].to_string(), inner[k + 1..].to_string()), _ => (inner.clone(), String::new()) };
                    let mut p = Parser { t: lex(&expr_s)?, p: 0 }; let ex = p.expr()?; let v = self.eval(&ex, h)?;
                    if let Some(prec) = spec.strip_prefix('.').and_then(|x| x.strip_suffix('f')).and_then(|x| x.parse::<usize>().ok()) { out.push_str(&format!("{:.*}", prec, v.num()?)); } else { out.push_str(&v.text()); }
                    i = j + 1; } else { out.push(ch[i]); i += 1; } }
                Ok(V::s(&out)) }
            Ex::Attr(obj, name) => { if matches!(&**obj, Ex::Name(m) if m == "math") { return match name.as_str() { "pi" => Ok(V::Float(std::f64::consts::PI)), "e" => Ok(V::Float(std::f64::consts::E)), _ => Err(format!("math.{} is not a value", name)) }; } let o = self.eval(obj, h)?; Ok(V::list(vec![V::s(&format!("<method {}>", name)), o])) }
            Ex::Call(f, args, kw) => {
                let mut av = Vec::new(); for a in args { av.push(self.eval(a, h)?); }
                let mut kv = Vec::new(); for (k, a) in kw { kv.push((k.clone(), self.eval(a, h)?)); }
                match &**f {
                    Ex::Attr(obj, name) => { let o = self.eval(obj, h)?; if let Some(r) = self.method(&o, name, &av)? { return Ok(r); } if matches!(&**obj, Ex::Name(m) if m == "math") { return self.builtin(name, &av, h); } Err(format!("unknown method '{}'", name)) }
                    Ex::Name(n) => {
                        if let Some(V::Func(d)) = self.get(n) { return self.call_def(&d, av, kv, h); }
                        self.builtin(n, &av, h)
                    }
                    other => { if let V::Func(d) = self.eval(other, h)? { self.call_def(&d, av, kv, h) } else { Err("object is not callable".into()) } }
                }
            }
        }
    }
    fn call_def(&mut self, d: &Rc<Def>, args: Vec<V>, kw: Vec<(String, V)>, h: &mut dyn Host) -> R<V> {
        if self.frames.len() > 60 { return Err("recursion too deep".into()); }
        let mut frame = HashMap::new();
        for (i, (pn, def)) in d.params.iter().enumerate() {
            let v = if i < args.len() { args[i].clone() } else if let Some((_, v)) = kw.iter().find(|(k, _)| k == pn) { v.clone() } else if let Some(de) = def { self.eval(de, h)? } else { return Err(format!("missing argument '{}' in {}()", pn, d.name)); };
            frame.insert(pn.clone(), v);
        }
        self.frames.push(frame); self.global_decl.push(vec![]);
        let r = self.exec_block(&d.body, h);
        self.frames.pop(); self.global_decl.pop();
        match r? { Flow::Return(v) => Ok(v), _ => Ok(V::None) }
    }
    fn method(&mut self, o: &V, name: &str, a: &[V]) -> R<Option<V>> {
        Ok(match (o, name) {
            (V::List(l), "append") => { l.borrow_mut().push(a.get(0).cloned().unwrap_or(V::None)); Some(V::None) }
            (V::List(l), "extend") => { if let Some(V::List(x)) = a.get(0) { let x = x.borrow().clone(); l.borrow_mut().extend(x); } Some(V::None) }
            (V::List(l), "pop") => { let mut l = l.borrow_mut(); let n = l.len(); if n == 0 { return Err("pop from empty list".into()); } let i = a.get(0).map(|v| v.num().unwrap_or(0.0) as i64).unwrap_or(-1); let i = if i < 0 { (n as i64 + i) as usize } else { i as usize }; Some(l.remove(i.min(n - 1))) }
            (V::List(l), "reverse") => { l.borrow_mut().reverse(); Some(V::None) }
            (V::List(l), "sort") => { l.borrow_mut().sort_by(|x, y| x.num().unwrap_or(0.0).partial_cmp(&y.num().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)); Some(V::None) }
            (V::Dict(d), "keys") => Some(V::list(d.borrow().iter().map(|(k, _)| k.clone()).collect())),
            (V::Dict(d), "values") => Some(V::list(d.borrow().iter().map(|(_, v)| v.clone()).collect())),
            (V::Dict(d), "items") => Some(V::list(d.borrow().iter().map(|(k, v)| V::list(vec![k.clone(), v.clone()])).collect())),
            (V::Dict(d), "get") => { let k = a.get(0).cloned().unwrap_or(V::None); Some(d.borrow().iter().find(|(x, _)| x.eq(&k)).map(|(_, v)| v.clone()).unwrap_or_else(|| a.get(1).cloned().unwrap_or(V::None))) }
            (V::Str(s), "upper") => Some(V::s(&s.to_uppercase())), (V::Str(s), "lower") => Some(V::s(&s.to_lowercase())),
            (V::Str(s), "split") => Some(V::list(match a.get(0) { Some(sep) => s.split(&sep.text()).map(V::s).collect(), None => s.split_whitespace().map(V::s).collect() })),
            (V::Str(s), "join") => None.or(Some(V::s(&{ let _ = s; String::new() }))).filter(|_| false),
            (V::Str(s), "startswith") => Some(V::Bool(s.starts_with(&a.get(0).map(|v| v.text()).unwrap_or_default()))),
            _ => None,
        }.or_else(|| if let (V::Str(sep), "join", Some(V::List(l))) = (o, name, a.get(0)) { Some(V::s(&l.borrow().iter().map(|x| x.text()).collect::<Vec<_>>().join(sep))) } else { None }))
    }
    fn builtin(&mut self, n: &str, a: &[V], h: &mut dyn Host) -> R<V> {
        let f = |i: usize| -> R<f64> { a.get(i).ok_or_else(|| format!("{}() is missing an argument", n))?.num() };
        match n {
            "print" => { self.out.push(a.iter().map(|v| v.text()).collect::<Vec<_>>().join(" ")); Ok(V::None) }
            "range" => { let (s, e, st) = match a.len() { 1 => (0.0, f(0)?, 1.0), 2 => (f(0)?, f(1)?, 1.0), _ => (f(0)?, f(1)?, f(2)?) }; if st == 0.0 { return Err("range() step is zero".into()); } let mut v = Vec::new(); let mut x = s; while (st > 0.0 && x < e) || (st < 0.0 && x > e) { v.push(V::Int(x as i64)); x += st; if v.len() > 1_000_000 { break; } } Ok(V::list(v)) }
            "len" => match a.get(0) { Some(V::List(l)) => Ok(V::Int(l.borrow().len() as i64)), Some(V::Str(s)) => Ok(V::Int(s.chars().count() as i64)), Some(V::Dict(d)) => Ok(V::Int(d.borrow().len() as i64)), _ => Err("len() needs a list, string or dict".into()) },
            "abs" => Ok(match a.get(0) { Some(V::Int(i)) => V::Int(i.abs()), _ => V::Float(f(0)?.abs()) }),
            "min" | "max" => { let items: Vec<V> = if a.len() == 1 { self.iter(&a[0])? } else { a.to_vec() }; if items.is_empty() { return Err(format!("{}() of empty sequence", n)); } let mut best = items[0].clone(); for x in &items[1..] { let better = if n == "min" { x.num()? < best.num()? } else { x.num()? > best.num()? }; if better { best = x.clone(); } } Ok(best) }
            "sum" => { let items = self.iter(&a[0])?; let mut s = 0.0; let mut all_int = true; for x in items { if !matches!(x, V::Int(_)) { all_int = false; } s += x.num()?; } Ok(if all_int { V::Int(s as i64) } else { V::Float(s) }) }
            "int" => Ok(match a.get(0) { Some(V::Str(s)) => V::Int(s.trim().parse::<f64>().map_err(|_| "invalid int".to_string())? as i64), _ => V::Int(f(0)? as i64) }),
            "float" => Ok(match a.get(0) { Some(V::Str(s)) => V::Float(s.trim().parse::<f64>().map_err(|_| "invalid float".to_string())?), _ => V::Float(f(0)?) }),
            "str" => Ok(V::s(&a.get(0).map(|v| v.text()).unwrap_or_default())),
            "list" => Ok(V::list(a.get(0).map(|v| self.iter(v)).transpose()?.unwrap_or_default())),
            "enumerate" => Ok(V::list(self.iter(&a[0])?.into_iter().enumerate().map(|(i, v)| V::list(vec![V::Int(i as i64), v])).collect())),
            "round" => { let p = a.get(1).map(|v| v.num().unwrap_or(0.0)).unwrap_or(0.0); let m = 10f64.powf(p); let r = (f(0)? * m).round() / m; Ok(if a.len() < 2 { V::Int(r as i64) } else { V::Float(r) }) }
            "sqrt" => Ok(V::Float(f(0)?.sqrt())), "sin" => Ok(V::Float(f(0)?.sin())), "cos" => Ok(V::Float(f(0)?.cos())), "tan" => Ok(V::Float(f(0)?.tan())),
            "atan2" => Ok(V::Float(f(0)?.atan2(f(1)?))), "radians" => Ok(V::Float(f(0)?.to_radians())), "degrees" => Ok(V::Float(f(0)?.to_degrees())),
            "random" => { self.steps += 0; Ok(V::Float(rand_f())) }
            "randint" => { let (lo, hi) = (f(0)?, f(1)?); Ok(V::Int((lo + (rand_f() * (hi - lo + 1.0)).floor()) as i64)) }
            _ => match h.call(n, a) { Some(r) => r, None => Err(format!("name '{}' is not defined", n)) },
        }
    }
}

fn rand_f() -> f64 {
    thread_local! { static S: std::cell::Cell<u64> = std::cell::Cell::new(0x2545F4914F6CDD1D); }
    S.with(|s| { let mut x = s.get(); x ^= x << 13; x ^= x >> 7; x ^= x << 17; s.set(x); (x >> 11) as f64 / (1u64 << 53) as f64 })
}
