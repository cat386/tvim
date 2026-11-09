//! Регулярные выражения в синтаксисе vim («magic»).
//!
//! Небольшой движок с возвратами (backtracking) для поиска `/`, `?`, `*`, `#`
//! и замены `:s`. Совпадение ищется внутри одной строки.
//!
//! Поддерживается:
//!
//! | Запись | Смысл |
//! |--------|-------|
//! | `.` | любой символ |
//! | `[abc]`, `[^a-z]` | класс символов |
//! | `*`, `\+`, `\=`, `\?` | 0+, 1+, 0..1 повторений |
//! | `\{n,m}`, `\{n}`, `\{-n,m}` | повторения (с `-` — нежадные) |
//! | `^`, `$` | начало / конец строки |
//! | `\<`, `\>` | начало / конец слова |
//! | `\(...\)`, `\%(...\)`, `\|` | группы и альтернатива |
//! | `\s \S \d \D \w \W \a \A \l \L \u \U \x \X \h` | классы |
//! | `\t`, `\e`, `\\`, `\.`, `\*`, `\[` ... | экранированные символы |
//! | `\c`, `\C` | не учитывать / учитывать регистр |
//! | `\v` | «very magic»: `( ) | + ? = { } < >` без обратной черты |

/// Узел выражения.
#[derive(Clone, Debug)]
enum Node {
    Char(char),
    Any,
    Class {
        items: Vec<ClassItem>,
        neg: bool,
    },
    Bol,
    Eol,
    WordStart,
    WordEnd,
    /// Группа: варианты и номер захвата (`None` — без захвата).
    Group(Vec<Vec<Node>>, Option<usize>),
    /// Повторение: узел, минимум, максимум, жадность.
    Repeat(Box<Node>, usize, Option<usize>, bool),
}

#[derive(Clone, Debug)]
enum ClassItem {
    Range(char, char),
    Named(char),
}

/// Захваченные группы: (начало, конец) в символах строки.
pub type Caps = Vec<Option<(usize, usize)>>;

/// Скомпилированное выражение.
#[derive(Clone, Debug)]
pub struct Regex {
    /// Корень: одна группа без захвата со всеми вариантами.
    root: Vec<Node>,
    ncaps: usize,
    /// Без учёта регистра.
    pub icase: bool,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn named_class(n: char, c: char) -> bool {
    match n {
        's' => c == ' ' || c == '\t',
        'S' => !(c == ' ' || c == '\t'),
        'd' => c.is_ascii_digit(),
        'D' => !c.is_ascii_digit(),
        'w' => c.is_ascii_alphanumeric() || c == '_',
        'W' => !(c.is_ascii_alphanumeric() || c == '_'),
        'a' => c.is_alphabetic(),
        'A' => !c.is_alphabetic(),
        'l' => c.is_lowercase(),
        'L' => !c.is_lowercase(),
        'u' => c.is_uppercase(),
        'U' => !c.is_uppercase(),
        'x' => c.is_ascii_hexdigit(),
        'X' => !c.is_ascii_hexdigit(),
        'h' => c.is_ascii_alphabetic() || c == '_',
        'k' | 'i' => is_word(c),
        _ => false,
    }
}

/// Перевести «very magic» (`\v`) в обычную запись.
fn from_very_magic(p: &[char]) -> Vec<char> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < p.len() {
        let c = p[i];
        if c == '\\' && i + 1 < p.len() {
            let n = p[i + 1];
            if "()|+?={}<>@%".contains(n) {
                out.push(n); // экранированный — буквально
            } else {
                out.push('\\');
                out.push(n);
            }
            i += 2;
            continue;
        }
        if "()|+?={}<>%".contains(c) {
            if c == '%' && i + 1 < p.len() && p[i + 1] == '(' {
                out.extend(['\\', '%', '\\', '(']);
                i += 2;
                continue;
            }
            out.push('\\');
        }
        out.push(c);
        i += 1;
    }
    out
}

struct Parser {
    p: Vec<char>,
    i: usize,
    ncaps: usize,
    icase: Option<bool>,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.p.get(self.i).copied()
    }
    fn peek2(&self) -> Option<char> {
        self.p.get(self.i + 1).copied()
    }

    /// Варианты, разделённые `\|`, до `\)` или конца.
    fn parse_alts(&mut self) -> Result<Vec<Vec<Node>>, String> {
        let mut alts = vec![self.parse_seq()?];
        while self.peek() == Some('\\') && self.peek2() == Some('|') {
            self.i += 2;
            alts.push(self.parse_seq()?);
        }
        Ok(alts)
    }

    fn parse_seq(&mut self) -> Result<Vec<Node>, String> {
        let mut seq: Vec<Node> = Vec::new();
        loop {
            let Some(c) = self.peek() else { break };
            if c == '\\' && matches!(self.peek2(), Some('|') | Some(')')) {
                break;
            }
            // Квантификаторы.
            if c == '*' && !seq.is_empty() {
                self.i += 1;
                self.quantify(&mut seq, 0, None, true)?;
                continue;
            }
            if c == '\\' {
                match self.peek2() {
                    Some('+') if !seq.is_empty() => {
                        self.i += 2;
                        self.quantify(&mut seq, 1, None, true)?;
                        continue;
                    }
                    Some('=') | Some('?') if !seq.is_empty() => {
                        self.i += 2;
                        self.quantify(&mut seq, 0, Some(1), true)?;
                        continue;
                    }
                    Some('{') if !seq.is_empty() => {
                        self.i += 2;
                        let (min, max, greedy) = self.parse_braces()?;
                        self.quantify(&mut seq, min, max, greedy)?;
                        continue;
                    }
                    _ => {}
                }
            }
            let node = self.parse_atom(seq.is_empty())?;
            if let Some(n) = node {
                seq.push(n);
            }
        }
        // `$` в конце ветки — конец строки.
        Ok(seq)
    }

    fn quantify(&mut self, seq: &mut Vec<Node>, min: usize, max: Option<usize>, greedy: bool) -> Result<(), String> {
        let last = seq.pop().unwrap();
        if matches!(last, Node::Bol | Node::Eol | Node::WordStart | Node::WordEnd) {
            return Err("E64: нечего повторять".into());
        }
        seq.push(Node::Repeat(Box::new(last), min, max, greedy));
        Ok(())
    }

    /// `\{n,m}` после `\{`: возвращает (мин, макс, жадность).
    fn parse_braces(&mut self) -> Result<(usize, Option<usize>, bool), String> {
        let mut greedy = true;
        if self.peek() == Some('-') {
            greedy = false;
            self.i += 1;
        }
        let mut body = String::new();
        loop {
            match self.peek() {
                Some('}') => {
                    self.i += 1;
                    break;
                }
                Some('\\') if self.peek2() == Some('}') => {
                    self.i += 2;
                    break;
                }
                Some(c) => {
                    body.push(c);
                    self.i += 1;
                }
                None => return Err("E554: незакрытая \\{".into()),
            }
        }
        let num = |s: &str| -> Result<Option<usize>, String> {
            let s = s.trim();
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse::<usize>().map(Some).map_err(|_| "E554: ошибка в \\{".to_string())
            }
        };
        match body.split_once(',') {
            None => {
                let n = num(&body)?;
                match n {
                    Some(n) => Ok((n, Some(n), greedy)),
                    None => Ok((0, None, greedy)),
                }
            }
            Some((a, b)) => Ok((num(a)?.unwrap_or(0), num(b)?, greedy)),
        }
    }

    fn parse_atom(&mut self, at_start: bool) -> Result<Option<Node>, String> {
        let c = self.peek().unwrap();
        self.i += 1;
        Ok(Some(match c {
            '.' => Node::Any,
            '^' if at_start => Node::Bol,
            '$' if self.at_branch_end() => Node::Eol,
            '[' => match self.parse_class()? {
                Some(n) => n,
                None => Node::Char('['),
            },
            '~' => Node::Char('~'),
            '\\' => {
                let Some(n) = self.peek() else { return Ok(Some(Node::Char('\\'))) };
                self.i += 1;
                match n {
                    '(' => {
                        self.ncaps += 1;
                        let idx = self.ncaps;
                        let alts = self.parse_alts()?;
                        self.close_group()?;
                        Node::Group(alts, Some(idx))
                    }
                    '%' if self.peek() == Some('(') => {
                        self.i += 1;
                        let alts = self.parse_alts()?;
                        self.close_group()?;
                        Node::Group(alts, None)
                    }
                    '<' => Node::WordStart,
                    '>' => Node::WordEnd,
                    'c' => {
                        self.icase = Some(true);
                        return Ok(None);
                    }
                    'C' => {
                        self.icase = Some(false);
                        return Ok(None);
                    }
                    't' => Node::Char('\t'),
                    'e' => Node::Char('\x1b'),
                    'n' | 'r' => Node::Char('\n'),
                    c if "sSdDwWaAlLuUxXhki".contains(c) => Node::Class { items: vec![ClassItem::Named(c)], neg: false },
                    c => Node::Char(c),
                }
            }
            c => Node::Char(c),
        }))
    }

    fn at_branch_end(&self) -> bool {
        match self.peek() {
            None => true,
            Some('\\') => matches!(self.peek2(), Some('|') | Some(')')),
            _ => false,
        }
    }

    fn close_group(&mut self) -> Result<(), String> {
        if self.peek() == Some('\\') && self.peek2() == Some(')') {
            self.i += 2;
            Ok(())
        } else {
            Err("E54: непарная \\(".into())
        }
    }

    /// Класс `[...]`; `None` — нет закрывающей скобки (тогда `[` — буква).
    fn parse_class(&mut self) -> Result<Option<Node>, String> {
        let start = self.i;
        let mut neg = false;
        if self.peek() == Some('^') {
            neg = true;
            self.i += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let Some(c) = self.peek() else {
                self.i = start;
                return Ok(None);
            };
            if c == ']' && !first {
                self.i += 1;
                break;
            }
            first = false;
            self.i += 1;
            let lo = if c == '\\' {
                match self.peek() {
                    Some(n) => {
                        self.i += 1;
                        match n {
                            't' => '\t',
                            'e' => '\x1b',
                            'n' => '\n',
                            n if "sSdDwWaAlLuUxXh".contains(n) => {
                                items.push(ClassItem::Named(n));
                                continue;
                            }
                            n => n,
                        }
                    }
                    None => '\\',
                }
            } else {
                c
            };
            if self.peek() == Some('-') && self.peek2().map(|x| x != ']').unwrap_or(false) {
                self.i += 1;
                let mut hi = self.peek().unwrap();
                self.i += 1;
                if hi == '\\' {
                    if let Some(n) = self.peek() {
                        hi = n;
                        self.i += 1;
                    }
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Range(lo, lo));
            }
        }
        Ok(Some(Node::Class { items, neg }))
    }
}

impl Regex {
    /// Скомпилировать выражение. `ignorecase`/`smartcase` — как одноимённые
    /// параметры vim (`\c`/`\C` в самом выражении важнее).
    pub fn new(pattern: &str, ignorecase: bool, smartcase: bool) -> Result<Regex, String> {
        let mut p: Vec<char> = pattern.chars().collect();
        if let Some(pos) = p.windows(2).position(|w| w == ['\\', 'v']) {
            let mut rest = from_very_magic(&p[pos + 2..]);
            p.truncate(pos);
            p.append(&mut rest);
        }
        let has_upper = {
            // Заглавные буквы, не являющиеся частью `\X`.
            let mut up = false;
            let mut i = 0;
            while i < p.len() {
                if p[i] == '\\' {
                    i += 2;
                    continue;
                }
                if p[i].is_uppercase() {
                    up = true;
                }
                i += 1;
            }
            up
        };
        let mut parser = Parser { p, i: 0, ncaps: 0, icase: None };
        let alts = parser.parse_alts()?;
        if parser.i < parser.p.len() {
            return Err("E55: непарная \\)".into());
        }
        let icase = parser.icase.unwrap_or(ignorecase && !(smartcase && has_upper));
        Ok(Regex { root: vec![Node::Group(alts, None)], ncaps: parser.ncaps, icase })
    }

    /// Регулярное выражение, совпадающее с текстом буквально.
    pub fn literal(text: &str, icase: bool) -> Regex {
        let seq = text.chars().map(Node::Char).collect();
        Regex { root: seq, ncaps: 0, icase }
    }

    /// Совпадение целого слова (`*` и `#` в vim).
    pub fn word(word: &str, icase: bool) -> Regex {
        let mut seq = vec![Node::WordStart];
        seq.extend(word.chars().map(Node::Char));
        seq.push(Node::WordEnd);
        Regex { root: seq, ncaps: 0, icase }
    }

    fn eq(&self, a: char, b: char) -> bool {
        if self.icase {
            a == b || a.to_lowercase().eq(b.to_lowercase())
        } else {
            a == b
        }
    }

    fn class_match(&self, items: &[ClassItem], neg: bool, c: char) -> bool {
        let hit = items.iter().any(|it| match *it {
            ClassItem::Named(n) => named_class(n, c),
            ClassItem::Range(lo, hi) => {
                if lo <= c && c <= hi {
                    true
                } else if self.icase {
                    let l: char = c.to_lowercase().next().unwrap_or(c);
                    let u: char = c.to_uppercase().next().unwrap_or(c);
                    (lo <= l && l <= hi) || (lo <= u && u <= hi)
                } else {
                    false
                }
            }
        });
        hit != neg
    }

    /// Совпадение одиночного (не составного) узла: новая позиция.
    fn single(&self, n: &Node, s: &[char], pos: usize) -> Option<usize> {
        match n {
            Node::Char(c) => (pos < s.len() && self.eq(s[pos], *c)).then_some(pos + 1),
            Node::Any => (pos < s.len()).then_some(pos + 1),
            Node::Class { items, neg } => (pos < s.len() && self.class_match(items, *neg, s[pos])).then_some(pos + 1),
            Node::Bol => (pos == 0).then_some(pos),
            Node::Eol => (pos == s.len()).then_some(pos),
            Node::WordStart => {
                let after = pos < s.len() && is_word(s[pos]);
                let before = pos > 0 && is_word(s[pos - 1]);
                (after && !before).then_some(pos)
            }
            Node::WordEnd => {
                let after = pos < s.len() && is_word(s[pos]);
                let before = pos > 0 && is_word(s[pos - 1]);
                (before && !after).then_some(pos)
            }
            Node::Group(..) | Node::Repeat(..) => None,
        }
    }

    /// Сопоставить последовательность с продолжением `k`.
    fn m(&self, seq: &[Node], s: &[char], pos: usize, caps: &mut Caps, k: &mut dyn FnMut(usize, &mut Caps) -> bool) -> bool {
        let Some((node, rest)) = seq.split_first() else {
            return k(pos, caps);
        };
        match node {
            Node::Group(alts, idx) => {
                for alt in alts {
                    let saved = caps.clone();
                    let ok = self.m(alt, s, pos, caps, &mut |p, caps: &mut Caps| {
                        let old = idx.map(|i| caps[i]);
                        if let Some(i) = idx {
                            caps[*i] = Some((pos, p));
                        }
                        if self.m(rest, s, p, caps, k) {
                            return true;
                        }
                        if let (Some(i), Some(o)) = (idx, old) {
                            caps[*i] = o;
                        }
                        false
                    });
                    if ok {
                        return true;
                    }
                    *caps = saved;
                }
                false
            }
            Node::Repeat(inner, min, max, greedy) => self.rep(inner, *min, *max, *greedy, 0, rest, s, pos, caps, k),
            n => match self.single(n, s, pos) {
                Some(p) => self.m(rest, s, p, caps, k),
                None => false,
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rep(
        &self,
        inner: &Node,
        min: usize,
        max: Option<usize>,
        greedy: bool,
        count: usize,
        rest: &[Node],
        s: &[char],
        pos: usize,
        caps: &mut Caps,
        k: &mut dyn FnMut(usize, &mut Caps) -> bool,
    ) -> bool {
        let can_more = max.map(|m| count < m).unwrap_or(true);
        let more = |caps: &mut Caps, k: &mut dyn FnMut(usize, &mut Caps) -> bool| -> bool {
            if !can_more {
                return false;
            }
            self.m(std::slice::from_ref(inner), s, pos, caps, &mut |p, caps: &mut Caps| {
                // Пустое повторение не продвигает — не зацикливаемся.
                if p == pos && count >= min {
                    return false;
                }
                self.rep(inner, min, max, greedy, count + 1, rest, s, p, caps, k)
            })
        };
        if greedy {
            if more(caps, k) {
                return true;
            }
            count >= min && self.m(rest, s, pos, caps, k)
        } else {
            if count >= min && self.m(rest, s, pos, caps, k) {
                return true;
            }
            more(caps, k)
        }
    }

    /// Найти совпадение, начинающееся не раньше `start`:
    /// (начало, конец, группы).
    pub fn find_at(&self, s: &[char], start: usize) -> Option<(usize, usize, Caps)> {
        for from in start..=s.len() {
            let mut caps: Caps = vec![None; self.ncaps + 1];
            let mut end = None;
            let mut found_caps = None;
            let ok = self.m(&self.root, s, from, &mut caps, &mut |p, c: &mut Caps| {
                end = Some(p);
                found_caps = Some(c.clone());
                true
            });
            if ok {
                let e = end.unwrap();
                let mut c = found_caps.unwrap();
                c[0] = Some((from, e));
                return Some((from, e, c));
            }
        }
        None
    }

    /// Все непересекающиеся совпадения в строке.
    pub fn find_all(&self, s: &[char]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut pos = 0;
        while pos <= s.len() {
            match self.find_at(s, pos) {
                Some((a, b, _)) => {
                    out.push((a, b));
                    pos = if b > a { b } else { a + 1 };
                }
                None => break,
            }
        }
        out
    }

    /// Последнее совпадение, начинающееся строго левее `before`.
    pub fn find_before(&self, s: &[char], before: usize) -> Option<(usize, usize)> {
        let mut last = None;
        for (a, b) in self.find_all(s) {
            if a < before {
                last = Some((a, b));
            } else {
                break;
            }
        }
        last
    }
}

/// Подставить замену `:s`: `&` и `\0` — всё совпадение, `\1`..`\9` —
/// группы, `\r`/`\n` — перевод строки, `\t` — табуляция, `\u`/`\l` —
/// следующую букву в верхний/нижний регистр, `\U`/`\L` ... `\E` — до конца.
pub fn expand_replacement(rep: &str, line: &[char], caps: &Caps) -> String {
    #[derive(PartialEq, Clone, Copy)]
    enum Case {
        None,
        Upper,
        Lower,
    }
    let mut out = String::new();
    let mut one = Case::None;
    let mut all = Case::None;
    let push = |out: &mut String, c: char, one: &mut Case, all: Case| {
        let mode = if *one != Case::None { *one } else { all };
        *one = Case::None;
        match mode {
            Case::Upper => out.extend(c.to_uppercase()),
            Case::Lower => out.extend(c.to_lowercase()),
            Case::None => out.push(c),
        }
    };
    let group = |i: usize| -> String { caps.get(i).copied().flatten().map(|(a, b)| line[a..b].iter().collect()).unwrap_or_default() };
    let r: Vec<char> = rep.chars().collect();
    let mut i = 0;
    while i < r.len() {
        let c = r[i];
        if c == '&' {
            for ch in group(0).chars() {
                push(&mut out, ch, &mut one, all);
            }
        } else if c == '\\' && i + 1 < r.len() {
            i += 1;
            match r[i] {
                d @ '0'..='9' => {
                    for ch in group(d as usize - '0' as usize).chars() {
                        push(&mut out, ch, &mut one, all);
                    }
                }
                'n' | 'r' => out.push('\n'),
                't' => out.push('\t'),
                'u' => one = Case::Upper,
                'l' => one = Case::Lower,
                'U' => all = Case::Upper,
                'L' => all = Case::Lower,
                'E' | 'e' => all = Case::None,
                o => push(&mut out, o, &mut one, all),
            }
        } else {
            push(&mut out, c, &mut one, all);
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(p: &str, s: &str) -> Option<(usize, usize)> {
        let r = Regex::new(p, false, false).unwrap();
        let s: Vec<char> = s.chars().collect();
        r.find_at(&s, 0).map(|(a, b, _)| (a, b))
    }

    #[test]
    fn basics() {
        assert_eq!(f("abc", "xxabcx"), Some((2, 5)));
        assert_eq!(f("a.c", "zzaXc"), Some((2, 5)));
        assert_eq!(f("^ab", "xab"), None);
        assert_eq!(f("ab$", "abab"), Some((2, 4)));
        assert_eq!(f("a*b", "aaab"), Some((0, 4)));
        assert_eq!(f("a\\+", "baaa"), Some((1, 4)));
        assert_eq!(f("colou\\=r", "color"), Some((0, 5)));
        assert_eq!(f("[0-9]\\{2,3}", "a12345"), Some((1, 4)));
        assert_eq!(f("\\<is\\>", "this is"), Some((5, 7)));
        assert_eq!(f("\\(foo\\|bar\\)baz", "xbarbaz"), Some((1, 7)));
        assert_eq!(f("\\d\\+", "abc 42"), Some((4, 6)));
        assert_eq!(f("\\vfoo(bar)+", "foobarbar"), Some((0, 9)));
        assert_eq!(f("a.\\{-}b", "aXbYb"), Some((0, 3)));
        assert_eq!(f("[^ ]\\+", "  кот"), Some((2, 5)));
        assert_eq!(f("a$b", "a$b"), Some((0, 3)));
    }

    #[test]
    fn case() {
        let s: Vec<char> = "Hello".chars().collect();
        assert!(Regex::new("hello", true, true).unwrap().find_at(&s, 0).is_some());
        assert!(Regex::new("hEllo", true, true).unwrap().find_at(&s, 0).is_none());
        assert!(Regex::new("hello\\c", false, false).unwrap().find_at(&s, 0).is_some());
        let r: Vec<char> = "ПРИВЕТ".chars().collect();
        assert!(Regex::new("привет", true, false).unwrap().find_at(&r, 0).is_some());
    }

    #[test]
    fn replace() {
        let r = Regex::new("\\(\\w\\+\\) \\(\\w\\+\\)", false, false).unwrap();
        let s: Vec<char> = "hello world".chars().collect();
        let (_, _, caps) = r.find_at(&s, 0).unwrap();
        assert_eq!(expand_replacement("\\2 \\u\\1 [&]", &s, &caps), "world Hello [hello world]");
        assert_eq!(expand_replacement("\\U\\1\\E!", &s, &caps), "HELLO!");
    }
}
