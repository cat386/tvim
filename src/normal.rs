//! Обычный и визуальный режимы: разбор команд, движения, операторы,
//! текстовые объекты.
//!
//! Команда набирается по клавише; после каждой клавиши набранное
//! разбирается заново ([`parse`]): `["x][число]команда`, где команда — это
//! движение (`w`, `3j`, `f(`), оператор с движением или текстовым объектом
//! (`d2w`, `ci"`, `yap`, `>}`), удвоенный оператор (`dd`, `>>`, `gUU`) или
//! простая команда (`x`, `p`, `J`, `.`, `qa`, `@a`, `zz`…). Пока команда
//! не дописана, разбор возвращает «неполная» и редактор ждёт.

use tvision::{Ctx, Info};

use crate::buffer::Pos;
use crate::editor::{cmd, is_word_char, CmdKind, DotRec, Mode, VKind, VimEditor};
use crate::keys::VKey;

/// Движение курсора.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordFwd(bool),
    WordBack(bool),
    WordEnd(bool),
    WordEndBack(bool),
    LineStart,
    FirstNonBlank,
    LineEnd,
    LastNonBlank,
    Column,
    GotoLine,
    GotoFirst,
    ScreenTop,
    ScreenMiddle,
    ScreenBottom,
    /// f/F/t/T: символ, вперёд, «до символа».
    Find(char, bool, bool),
    /// `;` (false) и `,` (true).
    RepeatFind(bool),
    MatchPair,
    ParaFwd,
    ParaBack,
    /// n (false) и N (true).
    SearchNext(bool),
    /// `*` (true) и `#` (false).
    Star(bool),
    NextLine,
    PrevLine,
    /// `_`: текущая (+count-1) строка, первый непробельный.
    CurLine,
    /// `'x` (false) и `` `x `` (true).
    Mark(char, bool),
}

/// Вид движения для оператора.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MKind {
    Exclusive,
    Inclusive,
    Linewise,
}

/// Оператор.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
    Lower,
    Upper,
    Toggle,
}

/// Над чем работает оператор.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Motion(Motion),
    /// Текстовый объект: символ и «внутренний» (`i`).
    Object(char, bool),
    /// Удвоенный оператор: строки.
    Line,
}

/// Вид вставки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ins {
    I,
    A,
    BigI,
    BigA,
    O,
    BigO,
    Gi,
}

/// Простые команды.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum S {
    DelChar,
    DelCharBack,
    DelToEol,
    ChangeToEol,
    SubstChar,
    SubstLine,
    YankLine,
    Put(bool),
    Join(bool),
    Undo,
    Redo,
    Dot,
    Insert(Ins),
    ReplaceMode,
    Visual(VKind),
    ToggleCase,
    ReplaceChar(char),
    Reselect,
    WriteQuit,
    QuitNoSave,
    Scroll(char),
    SetMark(char),
    Record(char),
    StopRecord,
    Play(char),
    PageDown,
    PageUp,
    HalfDown,
    HalfUp,
    LineDown,
    LineUp,
    Colon,
    SearchFwd,
    SearchBack,
    FileInfo,
    Incr(bool),
    RepeatSub,
    Esc,
    WinNext,
    WinPrev,
    WinClose,
    WinNew,
    GotoFile,
    // Визуальный режим.
    VSwap(bool),
    VOp(Op, bool),
    VJoin(bool),
    VReplace(char),
    VPut,
    VInsert(bool),
    VObject(char, bool),
    /// Оператор с поиском: `d/образец`, `c?образец`.
    OpSearch(Op, bool),
}

/// Действие разобранной команды.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Move(Motion),
    Op(Op, Target, Option<usize>),
    Simple(S),
}

/// Разобранная команда.
#[derive(Clone, Copy, Debug)]
pub struct Cmd {
    pub reg: Option<char>,
    pub count: Option<usize>,
    pub act: Act,
    /// С какой клавиши начинается сама команда (после регистра и счётчика).
    pub body_start: usize,
}

/// Результат разбора.
pub enum Parse {
    Incomplete,
    Invalid,
    Done(Cmd),
}

/// Участок текста для оператора.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RKind {
    /// `[a, b)` посимвольно.
    Char,
    /// Строки `a.line..=b.line`.
    Line,
    /// Прямоугольник: экранные столбцы `l..=r` (`r == usize::MAX` — до конца строк).
    Block(usize, usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Range {
    pub a: Pos,
    pub b: Pos,
    pub kind: RKind,
}

fn valid_reg(c: char) -> bool {
    c.is_ascii_alphanumeric() || "\"-_+*%/:.".contains(c)
}

fn parse_count(keys: &[VKey], mut i: usize) -> (Option<usize>, usize) {
    let mut n: Option<usize> = None;
    while let Some(VKey::Char(c)) = keys.get(i) {
        let Some(d) = c.to_digit(10) else { break };
        if d == 0 && n.is_none() {
            break;
        }
        n = Some(n.unwrap_or(0).saturating_mul(10).saturating_add(d as usize).min(999_999));
        i += 1;
    }
    (n, i)
}

/// Разбор движения: `Err(true)` — неполное, `Err(false)` — не движение.
fn parse_motion(keys: &[VKey], i: usize) -> Result<(Motion, usize), bool> {
    let Some(&k) = keys.get(i) else { return Err(true) };
    let arg = |j: usize| -> Result<char, bool> {
        match keys.get(j) {
            None => Err(true),
            Some(VKey::Char(c)) => Ok(*c),
            Some(VKey::Tab) => Ok('\t'),
            _ => Err(false),
        }
    };
    let m = match k {
        VKey::Char('h') | VKey::Left | VKey::Backspace | VKey::Ctrl('h') => Motion::Left,
        VKey::Char('l') | VKey::Right | VKey::Char(' ') => Motion::Right,
        VKey::Char('j') | VKey::Down | VKey::Ctrl('j') | VKey::Ctrl('n') => Motion::Down,
        VKey::Char('k') | VKey::Up | VKey::Ctrl('p') => Motion::Up,
        VKey::Char('w') => Motion::WordFwd(false),
        VKey::Char('W') => Motion::WordFwd(true),
        VKey::Char('b') => Motion::WordBack(false),
        VKey::Char('B') => Motion::WordBack(true),
        VKey::Char('e') => Motion::WordEnd(false),
        VKey::Char('E') => Motion::WordEnd(true),
        VKey::Char('0') | VKey::Home => Motion::LineStart,
        VKey::Char('^') => Motion::FirstNonBlank,
        VKey::Char('$') | VKey::End => Motion::LineEnd,
        VKey::Char('|') => Motion::Column,
        VKey::Char('G') => Motion::GotoLine,
        VKey::Char('H') => Motion::ScreenTop,
        VKey::Char('M') => Motion::ScreenMiddle,
        VKey::Char('L') => Motion::ScreenBottom,
        VKey::Char(c @ ('f' | 'F' | 't' | 'T')) => {
            let ch = arg(i + 1)?;
            return Ok((Motion::Find(ch, c == 'f' || c == 't', c == 't' || c == 'T'), i + 2));
        }
        VKey::Char(';') => Motion::RepeatFind(false),
        VKey::Char(',') => Motion::RepeatFind(true),
        VKey::Char('%') => Motion::MatchPair,
        VKey::Char('}') => Motion::ParaFwd,
        VKey::Char('{') => Motion::ParaBack,
        VKey::Char('n') => Motion::SearchNext(false),
        VKey::Char('N') => Motion::SearchNext(true),
        VKey::Char('*') => Motion::Star(true),
        VKey::Char('#') => Motion::Star(false),
        VKey::Enter | VKey::Char('+') => Motion::NextLine,
        VKey::Char('-') => Motion::PrevLine,
        VKey::Char('_') => Motion::CurLine,
        VKey::Char(c @ ('\'' | '`')) => {
            let ch = arg(i + 1)?;
            return Ok((Motion::Mark(ch, c == '`'), i + 2));
        }
        VKey::Char('g') => {
            let m = match keys.get(i + 1) {
                None => return Err(true),
                Some(VKey::Char('g')) => Motion::GotoFirst,
                Some(VKey::Char('e')) => Motion::WordEndBack(false),
                Some(VKey::Char('E')) => Motion::WordEndBack(true),
                Some(VKey::Char('_')) => Motion::LastNonBlank,
                Some(VKey::Char('j')) | Some(VKey::Down) => Motion::Down,
                Some(VKey::Char('k')) | Some(VKey::Up) => Motion::Up,
                Some(VKey::Char('0')) => Motion::LineStart,
                Some(VKey::Char('$')) => Motion::LineEnd,
                Some(VKey::Char('m')) => Motion::ScreenMiddle,
                _ => return Err(false),
            };
            return Ok((m, i + 2));
        }
        _ => return Err(false),
    };
    Ok((m, i + 1))
}

/// Оператор в позиции `i`: (оператор, длина).
fn parse_op(keys: &[VKey], i: usize) -> Result<(Op, usize), bool> {
    match keys.get(i) {
        Some(VKey::Char('d')) => Ok((Op::Delete, 1)),
        Some(VKey::Char('c')) => Ok((Op::Change, 1)),
        Some(VKey::Char('y')) => Ok((Op::Yank, 1)),
        Some(VKey::Char('>')) => Ok((Op::Indent, 1)),
        Some(VKey::Char('<')) => Ok((Op::Outdent, 1)),
        Some(VKey::Char('g')) => match keys.get(i + 1) {
            None => Err(true),
            Some(VKey::Char('~')) => Ok((Op::Toggle, 2)),
            Some(VKey::Char('u')) => Ok((Op::Lower, 2)),
            Some(VKey::Char('U')) => Ok((Op::Upper, 2)),
            _ => Err(false),
        },
        _ => Err(false),
    }
}

const OBJECTS: &str = "wWsp()b{}B[]<>\"'`t";

/// Разобрать набранную команду.
pub fn parse(keys: &[VKey], visual: bool) -> Parse {
    let mut i = 0;
    let mut reg = None;
    if keys.first() == Some(&VKey::Char('"')) {
        match keys.get(1) {
            None => return Parse::Incomplete,
            Some(VKey::Char(c)) if valid_reg(*c) => {
                reg = Some(*c);
                i = 2;
            }
            _ => return Parse::Invalid,
        }
    }
    let (count, ni) = parse_count(keys, i);
    i = ni;
    let body_start = i;
    let Some(&k) = keys.get(i) else { return Parse::Incomplete };
    let done = |act: Act| Parse::Done(Cmd { reg, count, act, body_start });
    let simple = |s: S| Parse::Done(Cmd { reg, count, act: Act::Simple(s), body_start });
    let arg = |j: usize| -> Result<char, bool> {
        match keys.get(j) {
            None => Err(true),
            Some(VKey::Char(c)) => Ok(*c),
            Some(VKey::Enter) => Ok('\n'),
            Some(VKey::Tab) => Ok('\t'),
            _ => Err(false),
        }
    };

    if visual {
        let s = match k {
            VKey::Char(c @ ('i' | 'a')) => match arg(i + 1) {
                Err(true) => return Parse::Incomplete,
                Ok(o) if OBJECTS.contains(o) => S::VObject(o, c == 'i'),
                _ => return Parse::Invalid,
            },
            VKey::Char('o') => S::VSwap(false),
            VKey::Char('O') => S::VSwap(true),
            VKey::Char('d') | VKey::Char('x') | VKey::Del => S::VOp(Op::Delete, false),
            VKey::Char('X') | VKey::Char('D') => S::VOp(Op::Delete, true),
            VKey::Char('y') => S::VOp(Op::Yank, false),
            VKey::Char('Y') => S::VOp(Op::Yank, true),
            VKey::Char('c') | VKey::Char('s') => S::VOp(Op::Change, false),
            VKey::Char('C') | VKey::Char('S') | VKey::Char('R') => S::VOp(Op::Change, true),
            VKey::Char('>') => S::VOp(Op::Indent, false),
            VKey::Char('<') => S::VOp(Op::Outdent, false),
            VKey::Char('~') => S::VOp(Op::Toggle, false),
            VKey::Char('u') => S::VOp(Op::Lower, false),
            VKey::Char('U') => S::VOp(Op::Upper, false),
            VKey::Char('J') => S::VJoin(true),
            VKey::Char('r') => match arg(i + 1) {
                Err(true) => return Parse::Incomplete,
                Ok(c) => S::VReplace(c),
                Err(false) => return Parse::Invalid,
            },
            VKey::Char('p') | VKey::Char('P') => S::VPut,
            VKey::Char('I') => S::VInsert(false),
            VKey::Char('A') => S::VInsert(true),
            VKey::Char(':') => S::Colon,
            VKey::Char('/') => S::SearchFwd,
            VKey::Char('?') => S::SearchBack,
            VKey::Char('v') => S::Visual(VKind::Char),
            VKey::Char('V') => S::Visual(VKind::Line),
            VKey::Ctrl('v') | VKey::Ctrl('q') => S::Visual(VKind::Block),
            VKey::Esc | VKey::Ctrl('c') => S::Esc,
            VKey::Char('g') => match keys.get(i + 1) {
                None => return Parse::Incomplete,
                Some(VKey::Char('J')) => S::VJoin(false),
                Some(VKey::Char('~')) => S::VOp(Op::Toggle, false),
                Some(VKey::Char('u')) => S::VOp(Op::Lower, false),
                Some(VKey::Char('U')) => S::VOp(Op::Upper, false),
                _ => match parse_motion(keys, i) {
                    Ok((m, _)) => return done(Act::Move(m)),
                    Err(true) => return Parse::Incomplete,
                    Err(false) => return Parse::Invalid,
                },
            },
            _ => match parse_motion(keys, i) {
                Ok((m, _)) => return done(Act::Move(m)),
                Err(true) => return Parse::Incomplete,
                Err(false) => match k {
                    VKey::Ctrl('f') | VKey::PgDn => S::PageDown,
                    VKey::Ctrl('b') | VKey::PgUp => S::PageUp,
                    VKey::Ctrl('d') => S::HalfDown,
                    VKey::Ctrl('u') => S::HalfUp,
                    VKey::Ctrl('e') => S::LineDown,
                    VKey::Ctrl('y') => S::LineUp,
                    VKey::Char('z') => match keys.get(i + 1) {
                        None => return Parse::Incomplete,
                        Some(VKey::Char(c)) if "ztb.-".contains(*c) => S::Scroll(*c),
                        Some(VKey::Enter) => S::Scroll('t'),
                        _ => return Parse::Invalid,
                    },
                    _ => return Parse::Invalid,
                },
            },
        };
        return simple(s);
    }

    // ---- операторы ----
    match parse_op(keys, i) {
        Err(true) => return Parse::Incomplete,
        Ok((op, len)) => {
            let j = i + len;
            let (count2, j) = parse_count(keys, j);
            let Some(&t) = keys.get(j) else { return Parse::Incomplete };
            if t == VKey::Char('/') || t == VKey::Char('?') {
                return Parse::Done(Cmd { reg, count: count.or(count2), act: Act::Simple(S::OpSearch(op, t == VKey::Char('/'))), body_start });
            }
            // Удвоение: dd, cc, yy, >>, <<, g~~, g~g~, guu, gugu, gUU, gUgU.
            let last = match keys[i + len - 1] {
                VKey::Char(c) => c,
                _ => ' ',
            };
            if t == VKey::Char(last) {
                return done(Act::Op(op, Target::Line, count2));
            }
            if len == 2 && t == VKey::Char('g') {
                match keys.get(j + 1) {
                    None => return Parse::Incomplete,
                    Some(VKey::Char(c)) if *c == last => return done(Act::Op(op, Target::Line, count2)),
                    _ => {}
                }
            }
            if let VKey::Char(c @ ('i' | 'a')) = t {
                return match arg(j + 1) {
                    Err(true) => Parse::Incomplete,
                    Ok(o) if OBJECTS.contains(o) => done(Act::Op(op, Target::Object(o, c == 'i'), count2)),
                    _ => Parse::Invalid,
                };
            }
            return match parse_motion(keys, j) {
                Ok((m, _)) => done(Act::Op(op, Target::Motion(m), count2)),
                Err(true) => Parse::Incomplete,
                Err(false) => Parse::Invalid,
            };
        }
        Err(false) => {}
    }

    // ---- движения ----
    match parse_motion(keys, i) {
        Ok((m, _)) => return done(Act::Move(m)),
        Err(true) => return Parse::Incomplete,
        Err(false) => {}
    }

    // ---- простые команды ----
    let s = match k {
        VKey::Char('x') | VKey::Del => S::DelChar,
        VKey::Char('X') => S::DelCharBack,
        VKey::Char('D') => S::DelToEol,
        VKey::Char('C') => S::ChangeToEol,
        VKey::Char('s') => S::SubstChar,
        VKey::Char('S') => S::SubstLine,
        VKey::Char('Y') => S::YankLine,
        VKey::Char('p') => S::Put(false),
        VKey::Char('P') => S::Put(true),
        VKey::Char('J') => S::Join(true),
        VKey::Char('u') => S::Undo,
        VKey::Ctrl('r') => S::Redo,
        VKey::Char('.') => S::Dot,
        VKey::Char('i') | VKey::Ins => S::Insert(Ins::I),
        VKey::Char('a') => S::Insert(Ins::A),
        VKey::Char('I') => S::Insert(Ins::BigI),
        VKey::Char('A') => S::Insert(Ins::BigA),
        VKey::Char('o') => S::Insert(Ins::O),
        VKey::Char('O') => S::Insert(Ins::BigO),
        VKey::Char('R') => S::ReplaceMode,
        VKey::Char('v') => S::Visual(VKind::Char),
        VKey::Char('V') => S::Visual(VKind::Line),
        VKey::Ctrl('v') | VKey::Ctrl('q') => S::Visual(VKind::Block),
        VKey::Char('~') => S::ToggleCase,
        VKey::Char('r') => match arg(i + 1) {
            Err(true) => return Parse::Incomplete,
            Ok(c) => S::ReplaceChar(c),
            Err(false) => return Parse::Invalid,
        },
        VKey::Char('m') => match arg(i + 1) {
            Err(true) => return Parse::Incomplete,
            Ok(c) if c.is_ascii_alphabetic() => S::SetMark(c),
            _ => return Parse::Invalid,
        },
        VKey::Char('q') => {
            // Остановка записи разбирается редактором (он знает, идёт ли запись).
            match arg(i + 1) {
                Err(true) => return Parse::Incomplete,
                Ok(c) if c.is_ascii_alphanumeric() => S::Record(c),
                _ => return Parse::Invalid,
            }
        }
        VKey::Char('@') => match arg(i + 1) {
            Err(true) => return Parse::Incomplete,
            Ok(c) if c.is_ascii_alphanumeric() || c == '@' => S::Play(c),
            _ => return Parse::Invalid,
        },
        VKey::Char('Z') => match keys.get(i + 1) {
            None => return Parse::Incomplete,
            Some(VKey::Char('Z')) => S::WriteQuit,
            Some(VKey::Char('Q')) => S::QuitNoSave,
            _ => return Parse::Invalid,
        },
        VKey::Char('z') => match keys.get(i + 1) {
            None => return Parse::Incomplete,
            Some(VKey::Char(c)) if "ztb.-".contains(*c) => S::Scroll(*c),
            Some(VKey::Enter) => S::Scroll('t'),
            _ => return Parse::Invalid,
        },
        VKey::Char('g') => match keys.get(i + 1) {
            None => return Parse::Incomplete,
            Some(VKey::Char('J')) => S::Join(false),
            Some(VKey::Char('i')) => S::Insert(Ins::Gi),
            Some(VKey::Char('I')) => S::Insert(Ins::BigI),
            Some(VKey::Char('v')) => S::Reselect,
            Some(VKey::Char('t')) => S::WinNext,
            Some(VKey::Char('T')) => S::WinPrev,
            Some(VKey::Char('f')) => S::GotoFile,
            _ => return Parse::Invalid,
        },
        VKey::Ctrl('w') => match keys.get(i + 1) {
            None => return Parse::Incomplete,
            Some(VKey::Char('w')) | Some(VKey::Ctrl('w')) | Some(VKey::Char('l')) | Some(VKey::Char('j')) => S::WinNext,
            Some(VKey::Char('W')) | Some(VKey::Char('p')) | Some(VKey::Char('h')) | Some(VKey::Char('k')) => S::WinPrev,
            Some(VKey::Char('c')) | Some(VKey::Char('q')) => S::WinClose,
            Some(VKey::Char('n')) | Some(VKey::Char('s')) | Some(VKey::Char('v')) => S::WinNew,
            _ => return Parse::Invalid,
        },
        VKey::Ctrl('f') | VKey::PgDn => S::PageDown,
        VKey::Ctrl('b') | VKey::PgUp => S::PageUp,
        VKey::Ctrl('d') => S::HalfDown,
        VKey::Ctrl('u') => S::HalfUp,
        VKey::Ctrl('e') => S::LineDown,
        VKey::Ctrl('y') => S::LineUp,
        VKey::Char(':') => S::Colon,
        VKey::Char('/') => S::SearchFwd,
        VKey::Char('?') => S::SearchBack,
        VKey::Ctrl('g') => S::FileInfo,
        VKey::Ctrl('a') => S::Incr(true),
        VKey::Ctrl('x') => S::Incr(false),
        VKey::Char('&') => S::RepeatSub,
        VKey::Esc | VKey::Ctrl('c') => S::Esc,
        _ => return Parse::Invalid,
    };
    simple(s)
}

/// Команды, меняющие текст (их повторяет `.`).
fn is_change(act: &Act) -> bool {
    match act {
        Act::Op(op, ..) => *op != Op::Yank,
        Act::Simple(s) => matches!(
            s,
            S::DelChar
                | S::DelCharBack
                | S::DelToEol
                | S::ChangeToEol
                | S::SubstChar
                | S::SubstLine
                | S::Put(_)
                | S::Join(_)
                | S::Insert(_)
                | S::ReplaceMode
                | S::ToggleCase
                | S::ReplaceChar(_)
                | S::Incr(_)
        ),
        Act::Move(_) => false,
    }
}

impl VimEditor {
    /// Клавиша в обычном или визуальном режиме.
    pub(crate) fn normal_key(&mut self, k: VKey, ctx: &mut Ctx) {
        // Остановка записи макроса: «q» без набранной команды.
        if self.pending.is_empty() && k == VKey::Char('q') && self.recording.is_some() && !matches!(self.mode, Mode::Visual(_)) {
            let (r, mut keys) = self.recording.take().unwrap();
            keys.pop(); // сама «q»
            self.macros.insert(r, keys);
            self.message = None;
            return;
        }
        if self.pending.is_empty() && !matches!(k, VKey::Esc) {
            if !matches!(self.message, Some((ref m, true)) if m.starts_with("Поиск дошёл")) {
                self.message = None;
            }
        }
        self.pending.push(k);
        let visual = matches!(self.mode, Mode::Visual(_));
        match parse(&self.pending, visual) {
            Parse::Incomplete => {}
            Parse::Invalid => {
                self.pending.clear();
            }
            Parse::Done(c) => {
                let keys = std::mem::take(&mut self.pending);
                let was_normal = self.mode == Mode::Normal;
                self.exec_cmd(c, &keys, ctx);
                // Повтор `.`: запоминаем изменяющие команды обычного режима.
                if was_normal && is_change(&c.act) && !matches!(c.act, Act::Simple(S::Dot)) {
                    let rec = DotRec { reg: c.reg, count: c.count, body: keys[c.body_start..].to_vec(), insert: Vec::new() };
                    if matches!(self.mode, Mode::Insert | Mode::Replace) {
                        self.dot_pending = Some(rec);
                    } else {
                        self.last_change = Some(rec);
                    }
                }
                if !matches!(self.mode, Mode::Insert | Mode::Replace) {
                    self.commit();
                }
                // Ctrl-O: вернуться во вставку после одной команды.
                if self.one_shot && self.mode == Mode::Normal {
                    self.one_shot = false;
                    self.mode = Mode::Insert;
                    self.update_cursor_shape();
                } else if self.one_shot && self.mode != Mode::Normal && !matches!(self.mode, Mode::Visual(_) | Mode::Cmdline(_)) {
                    self.one_shot = false;
                }
            }
        }
    }

    /// Выйти из визуального режима, запомнив выделение.
    pub(crate) fn exit_visual(&mut self) {
        if let Mode::Visual(k) = self.mode {
            self.last_visual = Some((self.vstart, self.cur, k));
            self.mode = Mode::Normal;
        }
    }

    fn exec_cmd(&mut self, c: Cmd, keys: &[VKey], ctx: &mut Ctx) {
        let count = c.count.unwrap_or(1);
        match c.act {
            Act::Move(m) => {
                let keep_x = matches!(m, Motion::Up | Motion::Down);
                let before = self.cur;
                if let Some((p, _)) = self.eval_motion(m, c.count, false) {
                    if matches!(
                        m,
                        Motion::GotoLine
                            | Motion::GotoFirst
                            | Motion::SearchNext(_)
                            | Motion::Star(_)
                            | Motion::MatchPair
                            | Motion::Mark(..)
                            | Motion::ParaFwd
                            | Motion::ParaBack
                    ) {
                        self.last_jump = Some(before);
                    }
                    self.cur = p;
                    if !keep_x {
                        self.want_x = if m == Motion::LineEnd { Some(usize::MAX) } else { None };
                    }
                }
            }
            Act::Op(op, target, count2) => {
                let total = match (c.count, count2) {
                    (None, None) => None,
                    (a, b) => Some(a.unwrap_or(1) * b.unwrap_or(1)),
                };
                self.operator(op, target, total, c.reg, ctx);
            }
            Act::Simple(s) => self.simple(s, c, count, keys, ctx),
        }
    }

    // ------------------------------------------------------------------
    // Движения
    // ------------------------------------------------------------------

    fn char_at(&self, p: Pos) -> Option<char> {
        let l = self.buf.line(p.line);
        if p.col < l.len() {
            Some(l[p.col])
        } else if p.line + 1 < self.buf.line_count() {
            Some('\n')
        } else {
            None
        }
    }

    fn step(&self, p: Pos) -> Option<Pos> {
        if p.col < self.buf.line_len(p.line) {
            Some(Pos::new(p.line, p.col + 1))
        } else if p.line + 1 < self.buf.line_count() {
            Some(Pos::new(p.line + 1, 0))
        } else {
            None
        }
    }

    fn back(&self, p: Pos) -> Option<Pos> {
        if p.col > 0 {
            Some(Pos::new(p.line, p.col - 1))
        } else if p.line > 0 {
            Some(Pos::new(p.line - 1, self.buf.line_len(p.line - 1)))
        } else {
            None
        }
    }

    fn cls(c: char, big: bool) -> u8 {
        if c.is_whitespace() {
            0
        } else if big || is_word_char(c) {
            1
        } else {
            2
        }
    }

    fn word_fwd(&self, from: Pos, big: bool) -> Pos {
        let mut p = from;
        let c0 = self.char_at(p).map(|c| Self::cls(c, big)).unwrap_or(0);
        if c0 != 0 {
            while let Some(ch) = self.char_at(p) {
                if ch == '\n' || Self::cls(ch, big) != c0 {
                    break;
                }
                match self.step(p) {
                    Some(n) => p = n,
                    None => return self.buf.end(),
                }
            }
        }
        loop {
            match self.char_at(p) {
                None => return self.buf.end(),
                Some('\n') => {
                    if self.buf.line_len(p.line) == 0 && p != from {
                        return p;
                    }
                }
                Some(ch) if !ch.is_whitespace() => return p,
                _ => {}
            }
            match self.step(p) {
                Some(n) => p = n,
                None => return self.buf.end(),
            }
        }
    }

    fn word_end(&self, from: Pos, big: bool) -> Pos {
        let Some(mut p) = self.step(from) else { return from };
        while let Some(ch) = self.char_at(p) {
            if !(ch == '\n' || ch.is_whitespace()) {
                break;
            }
            match self.step(p) {
                Some(n) => p = n,
                None => return p,
            }
        }
        let c = self.char_at(p).map(|c| Self::cls(c, big)).unwrap_or(0);
        while let Some(n) = self.step(p) {
            match self.char_at(n) {
                Some(ch) if ch != '\n' && Self::cls(ch, big) == c => p = n,
                _ => break,
            }
        }
        p
    }

    fn word_back(&self, from: Pos, big: bool) -> Pos {
        let Some(mut p) = self.back(from) else { return from };
        loop {
            match self.char_at(p) {
                Some('\n') | None => {
                    if self.buf.line_len(p.line) == 0 {
                        return p;
                    }
                }
                Some(ch) if !ch.is_whitespace() => break,
                _ => {}
            }
            match self.back(p) {
                Some(n) => p = n,
                None => return p,
            }
        }
        let c = Self::cls(self.char_at(p).unwrap_or(' '), big);
        while let Some(q) = self.back(p) {
            match self.char_at(q) {
                Some(ch) if ch != '\n' && Self::cls(ch, big) == c => p = q,
                _ => break,
            }
        }
        p
    }

    fn word_end_back(&self, from: Pos, big: bool) -> Pos {
        let mut p = from;
        let c0 = self.char_at(p).map(|c| Self::cls(c, big)).unwrap_or(0);
        if c0 != 0 {
            while let Some(q) = self.back(p) {
                match self.char_at(q) {
                    Some(ch) if ch != '\n' && Self::cls(ch, big) == c0 => p = q,
                    _ => break,
                }
            }
        }
        let Some(mut p) = self.back(p) else { return Pos::new(0, 0) };
        loop {
            match self.char_at(p) {
                Some('\n') | None => {
                    if self.buf.line_len(p.line) == 0 {
                        return p;
                    }
                }
                Some(ch) if !ch.is_whitespace() => return p,
                _ => {}
            }
            match self.back(p) {
                Some(n) => p = n,
                None => return p,
            }
        }
    }

    fn blank(&self, l: usize) -> bool {
        self.buf.line(l).iter().all(|c| c.is_whitespace())
    }

    /// Парная скобка (с учётом вложенности, через строки).
    pub(crate) fn find_pair(&self, p: Pos, limit: usize) -> Option<Pos> {
        let c = *self.buf.line(p.line).get(p.col)?;
        let (open, close, fwd) = match c {
            '(' => ('(', ')', true),
            '[' => ('[', ']', true),
            '{' => ('{', '}', true),
            ')' => ('(', ')', false),
            ']' => ('[', ']', false),
            '}' => ('{', '}', false),
            _ => return None,
        };
        let mut depth = 0i32;
        let mut q = p;
        let mut steps = 0;
        loop {
            let ch = self.buf.line(q.line).get(q.col).copied();
            if ch == Some(open) {
                depth += if fwd { 1 } else { -1 };
            } else if ch == Some(close) {
                depth += if fwd { -1 } else { 1 };
            }
            if depth == 0 {
                return Some(q);
            }
            q = if fwd { self.step(q)? } else { self.back(q)? };
            steps += 1;
            if limit > 0 && steps > limit * 80 {
                return None;
            }
        }
    }

    /// Вычислить движение: целевая позиция и вид.
    pub(crate) fn eval_motion(&mut self, m: Motion, count: Option<usize>, for_op: bool) -> Option<(Pos, MKind)> {
        let n = count.unwrap_or(1).max(1);
        let cur = self.cur;
        let last = self.buf.line_count() - 1;
        let len = self.buf.line_len(cur.line);
        Some(match m {
            Motion::Left => {
                if cur.col == 0 {
                    return None;
                }
                (Pos::new(cur.line, cur.col.saturating_sub(n)), MKind::Exclusive)
            }
            Motion::Right => {
                let max = if for_op { len } else { len.saturating_sub(1) };
                if cur.col >= max && !for_op {
                    return None;
                }
                (Pos::new(cur.line, (cur.col + n).min(max)), MKind::Exclusive)
            }
            Motion::Up | Motion::Down => {
                let line = if m == Motion::Up {
                    if cur.line == 0 {
                        return None;
                    }
                    cur.line.saturating_sub(n)
                } else {
                    if cur.line >= last {
                        return None;
                    }
                    (cur.line + n).min(last)
                };
                let x = self.want_x.unwrap_or_else(|| self.col_to_x(cur.line, cur.col));
                self.want_x = Some(x);
                let col = if x == usize::MAX { self.buf.line_len(line).saturating_sub(1) } else { self.x_to_col(line, x) };
                (Pos::new(line, col), MKind::Linewise)
            }
            Motion::WordFwd(big) => {
                let mut p = cur;
                for i in 0..n {
                    let q = self.word_fwd(p, big);
                    // Оператор с «w» не уходит на следующую строку за последним словом.
                    if for_op && i == n - 1 && q.line > p.line {
                        let l = if q.col == self.first_nonblank(q.line) && q.line > cur.line { q.line - 1 } else { q.line };
                        let e = Pos::new(l, self.buf.line_len(l));
                        p = if e > cur { e } else { q };
                        break;
                    }
                    p = q;
                }
                (p, MKind::Exclusive)
            }
            Motion::WordBack(big) => {
                let mut p = cur;
                for _ in 0..n {
                    p = self.word_back(p, big);
                }
                (p, MKind::Exclusive)
            }
            Motion::WordEnd(big) => {
                let mut p = cur;
                for _ in 0..n {
                    p = self.word_end(p, big);
                }
                (p, MKind::Inclusive)
            }
            Motion::WordEndBack(big) => {
                let mut p = cur;
                for _ in 0..n {
                    p = self.word_end_back(p, big);
                }
                (p, MKind::Inclusive)
            }
            Motion::LineStart => (Pos::new(cur.line, 0), MKind::Exclusive),
            Motion::FirstNonBlank => (Pos::new(cur.line, self.first_nonblank(cur.line)), MKind::Exclusive),
            Motion::LineEnd => {
                let l = (cur.line + n - 1).min(last);
                let ll = self.buf.line_len(l);
                (Pos::new(l, ll.saturating_sub(1)), MKind::Inclusive)
            }
            Motion::LastNonBlank => {
                let l = (cur.line + n - 1).min(last);
                let line = self.buf.line(l);
                let c = line.iter().rposition(|c| !c.is_whitespace()).unwrap_or(0);
                (Pos::new(l, c), MKind::Inclusive)
            }
            Motion::Column => {
                let col = self.x_to_col(cur.line, n - 1);
                (Pos::new(cur.line, col), MKind::Exclusive)
            }
            Motion::GotoLine => {
                let l = count.map(|c| c.saturating_sub(1).min(last)).unwrap_or(last);
                (Pos::new(l, self.first_nonblank(l)), MKind::Linewise)
            }
            Motion::GotoFirst => {
                let l = count.map(|c| c.saturating_sub(1).min(last)).unwrap_or(0);
                (Pos::new(l, self.first_nonblank(l)), MKind::Linewise)
            }
            Motion::ScreenTop | Motion::ScreenMiddle | Motion::ScreenBottom => {
                let top = self.delta.y.max(0) as usize;
                let h = self.text_height() as usize;
                let bottom = (top + h - 1).min(last);
                let l = match m {
                    Motion::ScreenTop => (top + n - 1).min(bottom),
                    Motion::ScreenBottom => bottom.saturating_sub(n - 1).max(top),
                    _ => (top + bottom) / 2,
                };
                (Pos::new(l, self.first_nonblank(l)), MKind::Linewise)
            }
            Motion::Find(ch, fwd, till) => {
                self.last_find = Some((ch, fwd, till));
                (self.find_char(ch, fwd, till, n, false)?, if fwd { MKind::Inclusive } else { MKind::Exclusive })
            }
            Motion::RepeatFind(rev) => {
                let (ch, fwd, till) = self.last_find?;
                let fwd = fwd != rev;
                (self.find_char(ch, fwd, till, n, true)?, if fwd { MKind::Inclusive } else { MKind::Exclusive })
            }
            Motion::MatchPair => {
                if let Some(c) = count {
                    // N% — на N процентов файла.
                    let l = ((c.min(100) * (last + 1)).div_ceil(100)).saturating_sub(1);
                    return Some((Pos::new(l, self.first_nonblank(l)), MKind::Linewise));
                }
                let line = self.buf.line(cur.line);
                let start = (cur.col..line.len()).find(|&i| "()[]{}".contains(line[i]))?;
                (self.find_pair(Pos::new(cur.line, start), 0)?, MKind::Inclusive)
            }
            Motion::ParaFwd => {
                let mut l = cur.line;
                for _ in 0..n {
                    while l < last && self.blank(l) {
                        l += 1;
                    }
                    while l < last && !self.blank(l) {
                        l += 1;
                    }
                }
                let col = if l == last && !self.blank(l) { self.buf.line_len(l).saturating_sub(if for_op { 0 } else { 1 }) } else { 0 };
                let kind = if l == last && !self.blank(l) && for_op { MKind::Inclusive } else { MKind::Exclusive };
                (Pos::new(l, col), kind)
            }
            Motion::ParaBack => {
                let mut l = cur.line;
                for _ in 0..n {
                    while l > 0 && self.blank(l) {
                        l -= 1;
                    }
                    while l > 0 && !self.blank(l) {
                        l -= 1;
                    }
                }
                (Pos::new(l, 0), MKind::Exclusive)
            }
            Motion::SearchNext(rev) => (self.search_next(rev, n, !for_op).filter(|_| true)?, MKind::Exclusive),
            Motion::Star(fwd) => {
                let Some((w, start)) = self.word_under_cursor(false) else {
                    self.error("E348: Нет слова под курсором");
                    return None;
                };
                let pat = format!(
                    "\\<{}\\>",
                    w.chars().map(|c| if "\\.*[]~^$".contains(c) { format!("\\{}", c) } else { c.to_string() }).collect::<String>()
                );
                self.search = Some((pat, fwd));
                self.search_history.push(self.search.as_ref().unwrap().0.clone());
                // Начинаем от начала слова, чтобы # не находил то же слово.
                let save = self.cur;
                self.cur = Pos::new(cur.line, start);
                let r = self.search_next(false, n, !for_op);
                if r.is_none() || for_op {
                    self.cur = save;
                }
                (r?, MKind::Exclusive)
            }
            Motion::NextLine => {
                if cur.line >= last {
                    return None;
                }
                let l = (cur.line + n).min(last);
                (Pos::new(l, self.first_nonblank(l)), MKind::Linewise)
            }
            Motion::PrevLine => {
                if cur.line == 0 {
                    return None;
                }
                let l = cur.line.saturating_sub(n);
                (Pos::new(l, self.first_nonblank(l)), MKind::Linewise)
            }
            Motion::CurLine => {
                let l = (cur.line + n - 1).min(last);
                (Pos::new(l, self.first_nonblank(l)), MKind::Linewise)
            }
            Motion::Mark(c, exact) => {
                let p = match c {
                    '\'' | '`' => self.last_jump?,
                    '<' => self.last_visual.map(|(a, b, _)| a.min(b))?,
                    '>' => self.last_visual.map(|(a, b, _)| a.max(b))?,
                    '.' => self.last_change_pos?,
                    '^' => self.last_insert?,
                    c => match self.marks.get(&c) {
                        Some(p) => *p,
                        None => {
                            self.error("E20: Метка не установлена");
                            return None;
                        }
                    },
                };
                let p = self.buf.clamp(p);
                if exact {
                    (p, MKind::Exclusive)
                } else {
                    (Pos::new(p.line, self.first_nonblank(p.line)), MKind::Linewise)
                }
            }
        })
    }

    /// f/F/t/T в текущей строке.
    fn find_char(&self, ch: char, fwd: bool, till: bool, n: usize, repeat: bool) -> Option<Pos> {
        let line = self.buf.line(self.cur.line);
        let mut col = self.cur.col;
        for i in 0..n {
            // При повторе t/T не застреваем перед тем же символом.
            let skip = if till && repeat && i == 0 { 1 } else { 0 };
            if fwd {
                let from = col + 1 + skip;
                let p = (from..line.len()).find(|&k| line[k] == ch)?;
                col = if till { p - 1 } else { p };
            } else {
                let to = col.checked_sub(skip)?;
                let p = (0..to).rev().find(|&k| line[k] == ch)?;
                col = if till { p + 1 } else { p };
            }
        }
        Some(Pos::new(self.cur.line, col))
    }

    // ------------------------------------------------------------------
    // Текстовые объекты
    // ------------------------------------------------------------------

    pub(crate) fn text_object(&self, obj: char, inner: bool, count: usize) -> Option<Range> {
        let cur = self.cur;
        match obj {
            'w' | 'W' => {
                let big = obj == 'W';
                let line = self.buf.line(cur.line);
                if line.is_empty() {
                    return Some(Range { a: Pos::new(cur.line, 0), b: Pos::new(cur.line, 0), kind: RKind::Char });
                }
                let col = cur.col.min(line.len() - 1);
                let c = Self::cls(line[col], big);
                let mut a = col;
                while a > 0 && Self::cls(line[a - 1], big) == c {
                    a -= 1;
                }
                let mut b = col + 1;
                let mut k = 1;
                loop {
                    while b < line.len() && Self::cls(line[b], big) == Self::cls(line[b - 1], big) {
                        b += 1;
                    }
                    if k >= count || b >= line.len() {
                        break;
                    }
                    k += 1;
                    b += 1;
                }
                if !inner {
                    // «aw» — со следующими пробелами (или с предыдущими, если следующих нет).
                    let mut e = b;
                    while e < line.len() && line[e].is_whitespace() {
                        e += 1;
                    }
                    if e > b && c != 0 {
                        b = e;
                    } else {
                        while a > 0 && line[a - 1].is_whitespace() {
                            a -= 1;
                        }
                    }
                }
                Some(Range { a: Pos::new(cur.line, a), b: Pos::new(cur.line, b), kind: RKind::Char })
            }
            '"' | '\'' | '`' => {
                let line = self.buf.line(cur.line);
                let mut quotes = Vec::new();
                let mut i = 0;
                while i < line.len() {
                    if line[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if line[i] == obj {
                        quotes.push(i);
                    }
                    i += 1;
                }
                let mut pair = None;
                for w in quotes.chunks(2) {
                    if w.len() == 2 && cur.col <= w[1] {
                        pair = Some((w[0], w[1]));
                        break;
                    }
                }
                let (qa, qb) = pair?;
                if inner {
                    Some(Range { a: Pos::new(cur.line, qa + 1), b: Pos::new(cur.line, qb), kind: RKind::Char })
                } else {
                    let mut b = qb + 1;
                    while b < line.len() && line[b].is_whitespace() {
                        b += 1;
                    }
                    Some(Range { a: Pos::new(cur.line, qa), b: Pos::new(cur.line, b), kind: RKind::Char })
                }
            }
            '(' | ')' | 'b' | '{' | '}' | 'B' | '[' | ']' | '<' | '>' => {
                let (open, close) = match obj {
                    '(' | ')' | 'b' => ('(', ')'),
                    '{' | '}' | 'B' => ('{', '}'),
                    '[' | ']' => ('[', ']'),
                    _ => ('<', '>'),
                };
                // Ищем открывающую скобку слева (с учётом вложенности), count раз.
                let mut p = cur;
                let mut found = None;
                for _ in 0..count {
                    let mut depth = 0;
                    let mut q = p;
                    let first_here = self.buf.line(q.line).get(q.col) == Some(&open) && found.is_none();
                    if !first_here {
                        loop {
                            let Some(nq) = self.back(q) else { return None };
                            q = nq;
                            match self.buf.line(q.line).get(q.col) {
                                Some(&c) if c == close => depth += 1,
                                Some(&c) if c == open => {
                                    if depth == 0 {
                                        break;
                                    }
                                    depth -= 1;
                                }
                                _ => {}
                            }
                        }
                    }
                    found = Some(q);
                    p = q;
                }
                let a = found?;
                // Парная закрывающая.
                let mut depth = 0;
                let mut q = a;
                let b = loop {
                    match self.buf.line(q.line).get(q.col) {
                        Some(&c) if c == open => depth += 1,
                        Some(&c) if c == close => {
                            depth -= 1;
                            if depth == 0 {
                                break q;
                            }
                        }
                        _ => {}
                    }
                    q = self.step(q)?;
                };
                if inner {
                    let mut ia = Pos::new(a.line, a.col + 1);
                    let mut ib = b;
                    // Блок на отдельных строках: внутренность — целые строки.
                    if ia.col >= self.buf.line_len(a.line) && b.line > a.line + 1 && self.buf.line(b.line)[..b.col].iter().all(|c| c.is_whitespace())
                    {
                        ia = Pos::new(a.line + 1, 0);
                        ib = Pos::new(b.line - 1, 0);
                        return Some(Range { a: ia, b: ib, kind: RKind::Line });
                    }
                    if ia.col >= self.buf.line_len(a.line) && b.line > a.line {
                        ia = Pos::new(a.line + 1, 0);
                    }
                    Some(Range { a: ia, b: ib, kind: RKind::Char })
                } else {
                    Some(Range { a, b: Pos::new(b.line, b.col + 1), kind: RKind::Char })
                }
            }
            'p' => {
                let last = self.buf.line_count() - 1;
                let bl = self.blank(cur.line);
                let mut a = cur.line;
                while a > 0 && self.blank(a - 1) == bl {
                    a -= 1;
                }
                let mut b = cur.line;
                for k in 0..count {
                    if k > 0 && b < last {
                        b += 1;
                    }
                    let kind = self.blank(b);
                    while b < last && self.blank(b + 1) == kind {
                        b += 1;
                    }
                }
                if !inner {
                    if b < last {
                        let kind = self.blank(b + 1);
                        b += 1;
                        while b < last && self.blank(b + 1) == kind {
                            b += 1;
                        }
                    } else {
                        while a > 0 && self.blank(a - 1) {
                            a -= 1;
                        }
                    }
                }
                Some(Range { a: Pos::new(a, 0), b: Pos::new(b, 0), kind: RKind::Line })
            }
            's' => {
                // Предложение: до «. », «! », «? » или пустой строки (в пределах абзаца-строки).
                let line = self.buf.line(cur.line);
                if line.is_empty() {
                    return None;
                }
                let mut a = cur.col.min(line.len() - 1);
                while a > 0 && !(matches!(line[a - 1], '.' | '!' | '?') && line[a].is_whitespace()) {
                    a -= 1;
                }
                while a < line.len() && line[a].is_whitespace() && a < cur.col {
                    a += 1;
                }
                let mut b = cur.col;
                while b < line.len() && !matches!(line[b], '.' | '!' | '?') {
                    b += 1;
                }
                b = (b + 1).min(line.len());
                if !inner {
                    while b < line.len() && line[b].is_whitespace() {
                        b += 1;
                    }
                }
                Some(Range { a: Pos::new(cur.line, a), b: Pos::new(cur.line, b), kind: RKind::Char })
            }
            't' => {
                // Тег XML/HTML: ближайший охватывающий <tag>…</tag>.
                let text = self.buf.text();
                let chars: Vec<char> = text.chars().collect();
                let mut offs = 0usize;
                for l in 0..cur.line {
                    offs += self.buf.line_len(l) + 1;
                }
                let pos = offs + cur.col;
                let to_pos = |o: usize| -> Pos {
                    let mut l = 0;
                    let mut o = o;
                    while l < self.buf.line_count() && o > self.buf.line_len(l) {
                        o -= self.buf.line_len(l) + 1;
                        l += 1;
                    }
                    Pos::new(l, o)
                };
                let mut search_from = pos;
                loop {
                    // Открывающий тег левее.
                    let open = (0..=search_from.min(chars.len().saturating_sub(1)))
                        .rev()
                        .find(|&i| chars[i] == '<' && chars.get(i + 1).map(|c| c.is_alphabetic()).unwrap_or(false))?;
                    let name: String = chars[open + 1..].iter().take_while(|c| c.is_alphanumeric() || **c == '-' || **c == ':').collect();
                    let open_end = (open..chars.len()).find(|&i| chars[i] == '>')? + 1;
                    let close_pat: Vec<char> = format!("</{}>", name).chars().collect();
                    let open_pat: Vec<char> = format!("<{}", name).chars().collect();
                    let mut depth = 1;
                    let mut i = open_end;
                    let mut close = None;
                    while i < chars.len() {
                        if chars[i..].starts_with(&close_pat) {
                            depth -= 1;
                            if depth == 0 {
                                close = Some(i);
                                break;
                            }
                        } else if chars[i..].starts_with(&open_pat) && chars.get(i + open_pat.len()).map(|c| !c.is_alphanumeric()).unwrap_or(false) {
                            depth += 1;
                        }
                        i += 1;
                    }
                    if let Some(c) = close {
                        let close_end = c + close_pat.len();
                        if close_end > pos {
                            return Some(if inner {
                                Range { a: to_pos(open_end), b: to_pos(c), kind: RKind::Char }
                            } else {
                                Range { a: to_pos(open), b: to_pos(close_end), kind: RKind::Char }
                            });
                        }
                    }
                    if open == 0 {
                        return None;
                    }
                    search_from = open - 1;
                }
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------------
    // Операторы
    // ------------------------------------------------------------------

    /// Участок для оператора с движением/объектом.
    fn op_range(&mut self, op: Op, target: Target, count: Option<usize>) -> Option<Range> {
        let n = count.unwrap_or(1);
        let cur = self.cur;
        match target {
            Target::Line => {
                let last = self.buf.line_count() - 1;
                if cur.line + n - 1 > last && n > 1 {
                    // vim всё равно работает до конца файла.
                }
                Some(Range { a: Pos::new(cur.line, 0), b: Pos::new((cur.line + n - 1).min(last), 0), kind: RKind::Line })
            }
            Target::Object(o, inner) => self.text_object(o, inner, n),
            Target::Motion(m) => {
                // «cw» на непробельном символе — как «ce».
                let m = match (op, m) {
                    (Op::Change, Motion::WordFwd(big)) if self.char_at(cur).map(|c| !c.is_whitespace()).unwrap_or(false) => Motion::WordEnd(big),
                    _ => m,
                };
                let (t, kind) = if let Motion::WordEnd(big) = m {
                    // «ce»: от курсора до конца слова, даже если курсор на конце слова.
                    let mut p = cur;
                    let start_on_end = op == Op::Change;
                    for i in 0..n {
                        let on_word = self.char_at(p).map(|c| !c.is_whitespace() && c != '\n').unwrap_or(false);
                        let next_same = self
                            .step(p)
                            .and_then(|q| self.char_at(q))
                            .map(|c| c != '\n' && Self::cls(c, big) == Self::cls(self.char_at(p).unwrap_or(' '), big))
                            .unwrap_or(false);
                        if i == 0 && start_on_end && on_word && !next_same {
                            continue;
                        }
                        p = self.word_end(p, big);
                    }
                    (p, MKind::Inclusive)
                } else {
                    self.eval_motion(m, count, true)?
                };
                let (a, b) = if t < cur { (t, cur) } else { (cur, t) };
                Some(match kind {
                    MKind::Linewise => Range { a, b, kind: RKind::Line },
                    MKind::Inclusive => {
                        let b2 = if b.col < self.buf.line_len(b.line) { Pos::new(b.line, b.col + 1) } else { b };
                        Range { a, b: b2, kind: RKind::Char }
                    }
                    MKind::Exclusive => {
                        // Правило vim: исключающее движение, кончившееся в начале
                        // строки, заканчивается в конце предыдущей.
                        if b.col == 0 && b.line > a.line && !matches!(m, Motion::Mark(..)) {
                            let pl = b.line - 1;
                            let e = Pos::new(pl, self.buf.line_len(pl));
                            if a.col <= self.first_nonblank(a.line) && op != Op::Change {
                                Range { a: Pos::new(a.line, 0), b: Pos::new(pl, 0), kind: RKind::Line }
                            } else {
                                Range { a, b: e, kind: RKind::Char }
                            }
                        } else {
                            Range { a, b, kind: RKind::Char }
                        }
                    }
                })
            }
        }
    }

    fn operator(&mut self, op: Op, target: Target, count: Option<usize>, reg: Option<char>, ctx: &mut Ctx) {
        let Some(r) = self.op_range(op, target, count) else {
            return;
        };
        self.apply(op, r, reg, 1, ctx);
    }

    /// Текст участка и признак «построчный».
    pub(crate) fn range_text(&self, r: &Range) -> (String, bool) {
        match r.kind {
            RKind::Char => (self.buf.slice(r.a, r.b), false),
            RKind::Line => {
                let lines: Vec<String> = (r.a.line..=r.b.line).map(|l| self.buf.line_string(l)).collect();
                (lines.join("\n"), true)
            }
            RKind::Block(l, rr) => {
                let lines: Vec<String> = (r.a.line..=r.b.line)
                    .map(|li| {
                        let (ca, cb) = self.block_cols(li, l, rr);
                        self.buf.line(li)[ca..cb].iter().collect()
                    })
                    .collect();
                (lines.join("\n"), false)
            }
        }
    }

    /// Символы строки в экранных столбцах `l..=r`.
    pub(crate) fn block_cols(&self, li: usize, l: usize, r: usize) -> (usize, usize) {
        let len = self.buf.line_len(li);
        let ca = self.x_to_col(li, l).min(len);
        let cb = if r == usize::MAX { len } else { (self.x_to_col(li, r) + 1).min(len) };
        (ca, cb.max(ca))
    }

    /// Удалить строки `a..=b` целиком.
    pub(crate) fn delete_lines(&mut self, a: usize, b: usize) {
        let last = self.buf.line_count() - 1;
        if b < last {
            self.buf.delete(Pos::new(a, 0), Pos::new(b + 1, 0));
        } else if a > 0 {
            let pl = a - 1;
            let e = Pos::new(b, self.buf.line_len(b));
            self.buf.delete(Pos::new(pl, self.buf.line_len(pl)), e);
        } else {
            let e = Pos::new(b, self.buf.line_len(b));
            self.buf.delete(Pos::new(0, 0), e);
        }
    }

    /// Сдвинуть строки вправо/влево на `times` уровней.
    pub(crate) fn shift_lines(&mut self, a: usize, b: usize, right: bool, times: usize) {
        let sw = self.indent_params().1;
        for l in a..=b {
            if self.buf.line_len(l) == 0 {
                continue;
            }
            let w = self.indent_width(l);
            let nw = if right { w + sw * times } else { w.saturating_sub(sw * times) };
            let fnb = self.first_nonblank(l);
            let ind = self.make_indent(nw);
            self.buf.replace(Pos::new(l, 0), Pos::new(l, fnb), &ind);
        }
    }

    /// Применить оператор к участку.
    pub(crate) fn apply(&mut self, op: Op, r: Range, reg: Option<char>, times: usize, ctx: &mut Ctx) {
        if self.readonly && op != Op::Yank {
            self.error("E21: Изменения запрещены: файл только для чтения");
            return;
        }
        match op {
            Op::Yank => {
                let (text, lw) = self.range_text(&r);
                let lines = text.matches('\n').count() + 1;
                self.set_reg(reg, text, lw, true, ctx);
                if lines > 2 {
                    self.info(&format!("скопировано строк: {}", lines));
                }
                match r.kind {
                    RKind::Line => {
                        if r.a.line < self.cur.line {
                            self.cur = Pos::new(r.a.line, self.cur.col);
                        }
                    }
                    RKind::Block(l, _) => self.cur = Pos::new(r.a.line, self.x_to_col(r.a.line, l)),
                    RKind::Char => self.cur = r.a,
                }
            }
            Op::Delete => {
                let (text, lw) = self.range_text(&r);
                self.set_reg(reg, text, lw, false, ctx);
                match r.kind {
                    RKind::Line => {
                        self.delete_lines(r.a.line, r.b.line);
                        let l = r.a.line.min(self.buf.line_count() - 1);
                        self.cur = Pos::new(l, self.first_nonblank(l));
                        let n = r.b.line - r.a.line + 1;
                        if n > 2 {
                            self.info(&format!("удалено строк: {}", n));
                        }
                    }
                    RKind::Char => {
                        self.buf.delete(r.a, r.b);
                        self.cur = r.a;
                    }
                    RKind::Block(l, rr) => {
                        for li in r.a.line..=r.b.line {
                            let (ca, cb) = self.block_cols(li, l, rr);
                            self.buf.delete(Pos::new(li, ca), Pos::new(li, cb));
                        }
                        self.cur = Pos::new(r.a.line, self.x_to_col(r.a.line, l));
                    }
                }
            }
            Op::Change => {
                let (text, lw) = self.range_text(&r);
                self.set_reg(reg, text, lw, false, ctx);
                match r.kind {
                    RKind::Line => {
                        let ai = self.cfg.borrow().options.autoindent;
                        let ind = if ai { self.indent_of(r.a.line) } else { String::new() };
                        let e = Pos::new(r.b.line, self.buf.line_len(r.b.line));
                        self.buf.replace(Pos::new(r.a.line, 0), e, &ind);
                        self.cur = Pos::new(r.a.line, ind.chars().count());
                        self.start_insert(false);
                    }
                    RKind::Char => {
                        self.buf.delete(r.a, r.b);
                        self.cur = r.a;
                        self.start_insert(false);
                    }
                    RKind::Block(l, rr) => {
                        for li in r.a.line..=r.b.line {
                            let (ca, cb) = self.block_cols(li, l, rr);
                            self.buf.delete(Pos::new(li, ca), Pos::new(li, cb));
                        }
                        self.cur = Pos::new(r.a.line, self.x_to_col(r.a.line, l));
                        self.start_insert(false);
                        self.block_insert =
                            Some(crate::editor::BlockInsert { first: r.a.line, last: r.b.line, x: l, start: self.cur, append: false });
                    }
                }
            }
            Op::Indent | Op::Outdent => {
                self.shift_lines(r.a.line, r.b.line, op == Op::Indent, times.max(1));
                self.cur = Pos::new(r.a.line, self.first_nonblank(r.a.line));
                let n = r.b.line - r.a.line + 1;
                if n > 2 {
                    self.info(&format!("сдвинуто строк: {}", n));
                }
            }
            Op::Lower | Op::Upper | Op::Toggle => {
                let conv = |c: char| -> String {
                    match op {
                        Op::Lower => c.to_lowercase().collect(),
                        Op::Upper => c.to_uppercase().collect(),
                        _ => {
                            if c.is_uppercase() {
                                c.to_lowercase().collect()
                            } else {
                                c.to_uppercase().collect()
                            }
                        }
                    }
                };
                let lines: Vec<(usize, usize, usize)> = match r.kind {
                    RKind::Line => (r.a.line..=r.b.line).map(|l| (l, 0, self.buf.line_len(l))).collect(),
                    RKind::Char => (r.a.line..=r.b.line)
                        .map(|l| {
                            let s = if l == r.a.line { r.a.col } else { 0 };
                            let e = if l == r.b.line { r.b.col } else { self.buf.line_len(l) };
                            (l, s, e)
                        })
                        .collect(),
                    RKind::Block(bl, br) => (r.a.line..=r.b.line)
                        .map(|l| {
                            let (a, b) = self.block_cols(l, bl, br);
                            (l, a, b)
                        })
                        .collect(),
                };
                for (l, s, e) in lines {
                    if s >= e {
                        continue;
                    }
                    let old: String = self.buf.line(l)[s..e].iter().collect();
                    let new: String = old.chars().map(conv).collect();
                    if new != old {
                        self.buf.replace(Pos::new(l, s), Pos::new(l, e), &new);
                    }
                }
                self.cur = match r.kind {
                    RKind::Line => Pos::new(r.a.line, self.cur.col),
                    RKind::Block(l, _) => Pos::new(r.a.line, self.x_to_col(r.a.line, l)),
                    RKind::Char => r.a,
                };
            }
        }
    }

    /// Соединить `n` строк начиная с `line` (`spaces` — с пробелами, как J).
    pub(crate) fn join(&mut self, line: usize, n: usize, spaces: bool) {
        let n = n.max(2);
        let mut col = self.cur.col;
        for _ in 0..n - 1 {
            if line + 1 >= self.buf.line_count() {
                break;
            }
            let len = self.buf.line_len(line);
            let next = self.buf.line(line + 1).to_vec();
            let lead = if spaces { next.iter().take_while(|c| c.is_whitespace()).count() } else { 0 };
            let cur_line = self.buf.line(line);
            let ends_ws = cur_line.last().map(|c| c.is_whitespace()).unwrap_or(true);
            let sep = if !spaces || next.len() == lead || ends_ws || next.get(lead) == Some(&')') { "" } else { " " };
            self.buf.replace(Pos::new(line, len), Pos::new(line + 1, lead), sep);
            col = len;
        }
        self.cur = Pos::new(line, col);
    }

    /// Вставить из регистра (p/P).
    pub(crate) fn put(&mut self, before: bool, count: usize, reg: Option<char>, ctx: &mut Ctx) {
        let Some(r) = self.get_reg(reg, ctx) else {
            self.error("E353: Регистр пуст");
            return;
        };
        if self.readonly {
            self.error("E21: Изменения запрещены");
            return;
        }
        let count = count.max(1);
        if r.linewise {
            let text = vec![r.text.clone(); count].join("\n");
            let l = if before { self.cur.line } else { self.cur.line + 1 };
            if l >= self.buf.line_count() {
                let last = self.buf.line_count() - 1;
                let e = Pos::new(last, self.buf.line_len(last));
                self.buf.insert(e, &format!("\n{}", text));
            } else {
                self.buf.insert(Pos::new(l, 0), &format!("{}\n", text));
            }
            self.cur = Pos::new(l, self.first_nonblank(l));
        } else {
            let text = r.text.repeat(count);
            let len = self.buf.line_len(self.cur.line);
            let at = if before || len == 0 { self.cur } else { Pos::new(self.cur.line, (self.cur.col + 1).min(len)) };
            let end = self.buf.insert(at, &text);
            self.cur = if text.contains('\n') { at } else { Pos::new(end.line, end.col.saturating_sub(1)) };
        }
    }

    /// Ctrl-A / Ctrl-X: изменить число под курсором или правее.
    fn increment(&mut self, delta: i64) {
        let line = self.buf.line(self.cur.line).to_vec();
        let mut i = self.cur.col;
        // Курсор внутри числа — к его началу.
        while i > 0 && i < line.len() && line[i].is_ascii_hexdigit() && line[i - 1].is_ascii_hexdigit() {
            i -= 1;
        }
        let Some(start) = (i..line.len()).find(|&k| line[k].is_ascii_digit()) else {
            return;
        };
        let mut a = start;
        let hex = line[a] == '0' && line.get(a + 1).map(|c| *c == 'x' || *c == 'X').unwrap_or(false);
        let (value, b, neg) = if hex {
            let mut b = a + 2;
            while b < line.len() && line[b].is_ascii_hexdigit() {
                b += 1;
            }
            let s: String = line[a + 2..b].iter().collect();
            (i64::from_str_radix(&s, 16).unwrap_or(0), b, false)
        } else {
            let mut b = a;
            while b < line.len() && line[b].is_ascii_digit() {
                b += 1;
            }
            let neg = a > 0 && line[a - 1] == '-';
            if neg {
                a -= 1;
            }
            let s: String = line[a..b].iter().collect();
            (s.parse::<i64>().unwrap_or(0), b, neg)
        };
        let _ = neg;
        let nv = value.saturating_add(delta);
        let new = if hex {
            let digits = b - start - 2;
            let upper = line[start + 2..b].iter().any(|c| c.is_ascii_uppercase());
            let h = if upper { format!("{:X}", nv.max(0)) } else { format!("{:x}", nv.max(0)) };
            format!("{}{}{:0>w$}", line[start], line[start + 1], h, w = digits)
        } else {
            nv.to_string()
        };
        let end = self.buf.replace(Pos::new(self.cur.line, a), Pos::new(self.cur.line, b), &new);
        self.cur = Pos::new(end.line, end.col.saturating_sub(1));
    }

    // ------------------------------------------------------------------
    // Простые команды
    // ------------------------------------------------------------------

    fn simple(&mut self, s: S, c: Cmd, count: usize, _keys: &[VKey], ctx: &mut Ctx) {
        let cur = self.cur;
        let len = self.buf.line_len(cur.line);
        let last = self.buf.line_count() - 1;
        let editable = |me: &mut VimEditor| -> bool {
            if me.readonly {
                me.error("E21: Изменения запрещены: файл только для чтения");
                false
            } else {
                true
            }
        };
        match s {
            S::DelChar => {
                if len > 0 && editable(self) {
                    let r = Range { a: cur, b: Pos::new(cur.line, (cur.col + count).min(len)), kind: RKind::Char };
                    self.apply(Op::Delete, r, c.reg, 1, ctx);
                }
            }
            S::DelCharBack => {
                if cur.col > 0 && editable(self) {
                    let r = Range { a: Pos::new(cur.line, cur.col.saturating_sub(count)), b: cur, kind: RKind::Char };
                    self.apply(Op::Delete, r, c.reg, 1, ctx);
                }
            }
            S::DelToEol | S::ChangeToEol => {
                if !editable(self) {
                    return;
                }
                let el = (cur.line + count - 1).min(last);
                let r = Range { a: cur, b: Pos::new(el, self.buf.line_len(el)), kind: RKind::Char };
                self.apply(if s == S::DelToEol { Op::Delete } else { Op::Change }, r, c.reg, 1, ctx);
            }
            S::SubstChar => {
                if !editable(self) {
                    return;
                }
                let r = Range { a: cur, b: Pos::new(cur.line, (cur.col + count).min(len)), kind: RKind::Char };
                self.apply(Op::Change, r, c.reg, 1, ctx);
            }
            S::SubstLine => {
                if !editable(self) {
                    return;
                }
                let r = Range { a: Pos::new(cur.line, 0), b: Pos::new((cur.line + count - 1).min(last), 0), kind: RKind::Line };
                self.apply(Op::Change, r, c.reg, 1, ctx);
            }
            S::YankLine => {
                let r = Range { a: Pos::new(cur.line, 0), b: Pos::new((cur.line + count - 1).min(last), 0), kind: RKind::Line };
                self.apply(Op::Yank, r, c.reg, 1, ctx);
            }
            S::Put(before) => self.put(before, count, c.reg, ctx),
            S::Join(spaces) => {
                if editable(self) {
                    self.join(cur.line, count.max(2), spaces);
                }
            }
            S::Undo => {
                let mut done = 0;
                for _ in 0..count {
                    match self.buf.undo() {
                        Some(p) => {
                            self.cur = p;
                            done += 1;
                        }
                        None => break,
                    }
                }
                if done == 0 {
                    self.info("Самое раннее изменение");
                } else {
                    self.info(&format!("отменено изменений: {}", done));
                }
            }
            S::Redo => {
                let mut done = 0;
                for _ in 0..count {
                    match self.buf.redo() {
                        Some(p) => {
                            self.cur = p;
                            done += 1;
                        }
                        None => break,
                    }
                }
                if done == 0 {
                    self.info("Самое позднее изменение");
                } else {
                    self.info(&format!("повторено изменений: {}", done));
                }
            }
            S::Dot => {
                let Some(rec) = self.last_change.clone() else { return };
                if self.replay_depth > 10 {
                    return;
                }
                let mut keys = Vec::new();
                if let Some(r) = rec.reg {
                    keys.push(VKey::Char('"'));
                    keys.push(VKey::Char(r));
                }
                if let Some(n) = c.count.or(rec.count) {
                    keys.extend(n.to_string().chars().map(VKey::Char));
                }
                keys.extend(rec.body.iter().copied());
                keys.extend(rec.insert.iter().copied());
                self.replay_depth += 1;
                for k in keys {
                    self.process(k, ctx);
                }
                self.replay_depth -= 1;
                // Повтор сам становится последним изменением (с новым счётчиком).
                if c.count.is_some() {
                    if let Some(lc) = &mut self.last_change {
                        lc.count = c.count;
                    }
                }
            }
            S::Insert(kind) => {
                if !editable(self) {
                    return;
                }
                self.insert_repeat = count;
                self.insert_open_line = None;
                match kind {
                    Ins::I => {}
                    Ins::A => {
                        if len > 0 {
                            self.cur.col += 1;
                        }
                    }
                    Ins::BigI => {
                        self.cur.col =
                            if c.body_start + 1 < _keys.len() && _keys[c.body_start] == VKey::Char('g') { 0 } else { self.first_nonblank(cur.line) }
                    }
                    Ins::BigA => self.cur.col = len,
                    Ins::O => {
                        let ai = self.cfg.borrow().options.autoindent;
                        let mut ind = if ai { self.indent_of(cur.line) } else { String::new() };
                        let si = self.cfg.borrow().options.smartindent;
                        let t: String = self.buf.line(cur.line).iter().collect();
                        let python = self.lang.map(|l| l.id == "python").unwrap_or(false);
                        if si && (t.trim_end().ends_with('{') || (python && t.trim_end().ends_with(':'))) {
                            let sw = self.indent_params().1;
                            ind = self.make_indent(self.indent_width(cur.line) + sw);
                        }
                        let e = Pos::new(cur.line, len);
                        self.cur = self.buf.insert(e, &format!("\n{}", ind));
                        self.insert_open_line = Some(false);
                    }
                    Ins::BigO => {
                        let ai = self.cfg.borrow().options.autoindent;
                        let ind = if ai { self.indent_of(cur.line) } else { String::new() };
                        self.buf.insert(Pos::new(cur.line, 0), &format!("{}\n", ind));
                        self.cur = Pos::new(cur.line, ind.chars().count());
                        self.insert_open_line = Some(true);
                    }
                    Ins::Gi => {
                        if let Some(p) = self.last_insert {
                            self.cur = self.buf.clamp(p);
                        }
                    }
                }
                self.start_insert(false);
            }
            S::ReplaceMode => {
                if editable(self) {
                    self.insert_repeat = count;
                    self.start_insert(true);
                }
            }
            S::Visual(k) => {
                match self.mode {
                    Mode::Visual(cur_k) if cur_k == k => {
                        self.exit_visual();
                    }
                    Mode::Visual(_) => self.mode = Mode::Visual(k),
                    _ => {
                        self.vstart = self.cur;
                        self.mode = Mode::Visual(k);
                        if count > 1 && k != VKind::Line {
                            self.cur.col = (self.cur.col + count - 1).min(len.saturating_sub(1));
                        } else if count > 1 {
                            self.cur.line = (self.cur.line + count - 1).min(last);
                        }
                    }
                }
                if k != VKind::Block && self.want_x == Some(usize::MAX) {
                    self.want_x = None;
                }
            }
            S::ToggleCase => {
                if len > 0 && editable(self) {
                    let e = (cur.col + count).min(len);
                    let r = Range { a: cur, b: Pos::new(cur.line, e), kind: RKind::Char };
                    self.apply(Op::Toggle, r, None, 1, ctx);
                    self.cur = Pos::new(cur.line, e.min(len - 1));
                }
            }
            S::ReplaceChar(ch) => {
                if !editable(self) || cur.col + count > len {
                    return;
                }
                if ch == '\n' {
                    self.buf.replace(cur, Pos::new(cur.line, cur.col + count), "\n");
                    self.cur = Pos::new(cur.line + 1, 0);
                } else {
                    let s: String = std::iter::repeat_n(ch, count).collect();
                    self.buf.replace(cur, Pos::new(cur.line, cur.col + count), &s);
                    self.cur = Pos::new(cur.line, cur.col + count - 1);
                }
            }
            S::Reselect => {
                if let Some((a, b, k)) = self.last_visual {
                    self.vstart = self.buf.clamp(a);
                    self.cur = self.buf.clamp(b);
                    self.mode = Mode::Visual(k);
                }
            }
            S::WriteQuit => self.execute_ex("x", ctx),
            S::QuitNoSave => self.execute_ex("q!", ctx),
            S::Scroll(z) => {
                let h = self.text_height();
                let y = cur.line as i32;
                self.delta.y = match z {
                    'z' | '.' => y - h / 2,
                    't' => y,
                    _ => y - h + 1,
                }
                .max(0);
                if z == '.' || z == '-' {
                    self.cur.col = self.first_nonblank(cur.line);
                }
            }
            S::SetMark(m) => {
                self.marks.insert(m, cur);
            }
            S::Record(r) => {
                self.recording = Some((r.to_ascii_lowercase(), Vec::new()));
                self.info(&format!("запись @{}", r));
            }
            S::StopRecord => {}
            S::Play(r) => {
                let r = if r == '@' {
                    match self.last_macro {
                        Some(m) => m,
                        None => {
                            self.error("E748: Нет предыдущего макроса");
                            return;
                        }
                    }
                } else {
                    r.to_ascii_lowercase()
                };
                // Макрос можно взять и из текстового регистра.
                let keys = match self.macros.get(&r) {
                    Some(k) => k.clone(),
                    None => match self.registers.get(&r) {
                        Some(reg) => crate::keys::parse_keys(&reg.text, self.cfg.borrow().options.leader).unwrap_or_default(),
                        None => {
                            self.error(&format!("E354: Регистр {} пуст", r));
                            return;
                        }
                    },
                };
                self.last_macro = Some(r);
                if self.replay_depth > 20 {
                    return;
                }
                self.replay_depth += 1;
                for _ in 0..count {
                    for &k in &keys {
                        self.map_feed(k, ctx);
                    }
                }
                self.replay_depth -= 1;
            }
            S::PageDown | S::PageUp | S::HalfDown | S::HalfUp => {
                let h = self.text_height();
                let max_y = last as i32;
                let page = match s {
                    S::PageDown | S::PageUp => (h - 2).max(1) * count as i32,
                    _ => (h / 2).max(1),
                };
                let down = matches!(s, S::PageDown | S::HalfDown);
                if (down && cur.line == last) || (!down && cur.line == 0) {
                    return;
                }
                if down {
                    self.delta.y = (self.delta.y + page).min(max_y);
                } else {
                    self.delta.y = (self.delta.y - page).max(0);
                }
                if matches!(s, S::HalfDown | S::HalfUp) {
                    let l = if down { (cur.line + page as usize).min(last) } else { cur.line.saturating_sub(page as usize) };
                    self.cur = Pos::new(l, self.first_nonblank(l));
                }
                self.keep_cursor_in_view();
            }
            S::LineDown | S::LineUp => {
                let max_y = last as i32;
                self.delta.y = if s == S::LineDown { (self.delta.y + count as i32).min(max_y) } else { (self.delta.y - count as i32).max(0) };
                self.keep_cursor_in_view();
            }
            S::Colon => {
                let init = match self.mode {
                    Mode::Visual(_) => "'<,'>".to_string(),
                    _ if c.count.is_some() => {
                        if count > 1 {
                            format!(".,.+{}", count - 1)
                        } else {
                            ".".to_string()
                        }
                    }
                    _ => String::new(),
                };
                if let Mode::Visual(k) = self.mode {
                    self.last_visual = Some((self.vstart, self.cur, k));
                }
                self.open_cmdline(CmdKind::Ex, &init);
            }
            S::OpSearch(op, fwd) => {
                self.pending_search_count = c.count;
                self.op_after_search = Some((op, c.reg));
                self.open_cmdline(if fwd { CmdKind::SearchFwd } else { CmdKind::SearchBack }, "");
            }
            S::SearchFwd | S::SearchBack => {
                self.pending_search_count = c.count;
                self.open_cmdline(if s == S::SearchFwd { CmdKind::SearchFwd } else { CmdKind::SearchBack }, "");
            }
            S::FileInfo => {
                let n = self.buf.line_count();
                let pct = (cur.line + 1) * 100 / n;
                let m = format!(
                    "\"{}\"{} {} строк --{}%-- [{}, {}]",
                    self.title(),
                    if self.modified() { " [Изменён]" } else { "" },
                    n,
                    pct,
                    self.encoding.name(),
                    if self.crlf { "dos" } else { "unix" }
                );
                self.info(&m);
            }
            S::Incr(up) => {
                if editable(self) {
                    self.increment(if up { count as i64 } else { -(count as i64) });
                }
            }
            S::RepeatSub => self.execute_ex("s", ctx),
            S::Esc => {
                if matches!(self.mode, Mode::Visual(_)) {
                    self.exit_visual();
                }
            }
            S::WinNext => ctx.post_command(tvision::cm::NEXT, Info::None),
            S::WinPrev => ctx.post_command(tvision::cm::PREV, Info::None),
            S::WinClose => self.execute_ex("q", ctx),
            S::WinNew => ctx.post_command(cmd::EDIT_FILE, Info::Str(String::new())),
            S::GotoFile => {
                let line = self.buf.line(cur.line);
                let isf = |c: char| !c.is_whitespace() && !"\"'<>()[]{},;|".contains(c);
                let mut a = cur.col.min(line.len());
                while a > 0 && isf(line[a - 1]) {
                    a -= 1;
                }
                let mut b = cur.col;
                while b < line.len() && isf(line[b]) {
                    b += 1;
                }
                let name: String = line[a..b].iter().collect();
                if name.is_empty() {
                    self.error("E446: Нет имени файла под курсором");
                } else {
                    // Путь относительно каталога текущего файла.
                    let base = self.file.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                    let p = base.join(&name);
                    let p = if p.exists() { p } else { std::path::PathBuf::from(&name) };
                    ctx.post_command(cmd::EDIT_FILE, Info::Str(p.display().to_string()));
                }
            }
            // ---- визуальный режим ----
            S::VSwap(other) => {
                if let Mode::Visual(VKind::Block) = self.mode {
                    if other {
                        // O в блоке: другой угол в той же строке.
                        let xa = self.col_to_x(self.vstart.line, self.vstart.col);
                        let xb = self.col_to_x(cur.line, cur.col);
                        let vs = self.vstart;
                        self.vstart = Pos::new(vs.line, self.x_to_col(vs.line, xb));
                        self.cur = Pos::new(cur.line, self.x_to_col(cur.line, xa));
                        return;
                    }
                }
                std::mem::swap(&mut self.vstart, &mut self.cur);
            }
            S::VObject(o, inner) => {
                if let Some(r) = self.text_object(o, inner, count) {
                    match r.kind {
                        RKind::Line => {
                            self.vstart = Pos::new(r.a.line, 0);
                            self.cur = Pos::new(r.b.line, 0);
                            self.mode = Mode::Visual(VKind::Line);
                        }
                        _ => {
                            self.vstart = r.a;
                            self.cur = self.back(r.b).unwrap_or(r.b).max(r.a);
                            if self.mode == Mode::Visual(VKind::Line) {
                                self.mode = Mode::Visual(VKind::Char);
                            }
                        }
                    }
                }
            }
            S::VOp(op, force_line) => {
                let Some(r) = self.visual_as_range(force_line) else { return };
                self.exit_visual();
                if op == Op::Change && matches!(r.kind, RKind::Block(..)) {
                    self.apply(op, r, c.reg, 1, ctx);
                    return;
                }
                self.apply(op, r, c.reg, count, ctx);
            }
            S::VJoin(spaces) => {
                let Some(r) = self.visual_as_range(true) else { return };
                self.exit_visual();
                if editable(self) {
                    self.join(r.a.line, (r.b.line - r.a.line + 1).max(2), spaces);
                }
            }
            S::VReplace(ch) => {
                let Some(r) = self.visual_as_range(false) else { return };
                self.exit_visual();
                if !editable(self) {
                    return;
                }
                let lines: Vec<(usize, usize, usize)> = match r.kind {
                    RKind::Line => (r.a.line..=r.b.line).map(|l| (l, 0, self.buf.line_len(l))).collect(),
                    RKind::Char => (r.a.line..=r.b.line)
                        .map(|l| (l, if l == r.a.line { r.a.col } else { 0 }, if l == r.b.line { r.b.col } else { self.buf.line_len(l) }))
                        .collect(),
                    RKind::Block(bl, br) => (r.a.line..=r.b.line)
                        .map(|l| {
                            let (a, b) = self.block_cols(l, bl, br);
                            (l, a, b)
                        })
                        .collect(),
                };
                for (l, a, b) in lines {
                    if b > a {
                        let s: String = std::iter::repeat_n(ch, b - a).collect();
                        self.buf.replace(Pos::new(l, a), Pos::new(l, b), &s);
                    }
                }
                self.cur = r.a;
            }
            S::VPut => {
                let Some(r) = self.visual_as_range(false) else { return };
                self.exit_visual();
                if !editable(self) {
                    return;
                }
                let Some(reg) = self.get_reg(c.reg, ctx) else {
                    self.error("E353: Регистр пуст");
                    return;
                };
                // Удаляем выделение (без записи в регистр) и вставляем на его место.
                match r.kind {
                    RKind::Line => {
                        let e = Pos::new(r.b.line, self.buf.line_len(r.b.line));
                        let text = reg.text.clone();
                        self.buf.replace(Pos::new(r.a.line, 0), e, &text);
                        self.cur = Pos::new(r.a.line, self.first_nonblank(r.a.line));
                    }
                    _ => {
                        self.apply(Op::Delete, r, Some('_'), 1, ctx);
                        if reg.linewise {
                            let at = self.cur;
                            let end = self.buf.insert(at, &format!("\n{}\n", reg.text));
                            self.cur = Pos::new(at.line + 1, 0);
                            let _ = end;
                        } else {
                            let end = self.buf.insert(self.cur, &reg.text);
                            self.cur = Pos::new(end.line, end.col.saturating_sub(1));
                        }
                    }
                }
            }
            S::VInsert(append) => {
                let Some(r) = self.visual_as_range(false) else { return };
                let kind = self.mode;
                self.exit_visual();
                if !editable(self) {
                    return;
                }
                match (kind, r.kind) {
                    (_, RKind::Block(l, rr)) => {
                        let x = if append {
                            if rr == usize::MAX {
                                usize::MAX
                            } else {
                                rr + 1
                            }
                        } else {
                            l
                        };
                        let first = r.a.line;
                        let col = if x == usize::MAX {
                            self.buf.line_len(first)
                        } else {
                            let lw = self.col_to_x(first, self.buf.line_len(first));
                            if lw < x {
                                let pad = " ".repeat(x - lw);
                                let len = self.buf.line_len(first);
                                self.buf.insert(Pos::new(first, len), &pad);
                            }
                            self.x_to_col(first, x)
                        };
                        self.cur = Pos::new(first, col);
                        self.start_insert(false);
                        self.block_insert = Some(crate::editor::BlockInsert { first, last: r.b.line, x, start: self.cur, append });
                    }
                    (_, RKind::Line) => {
                        self.cur =
                            if append { Pos::new(r.b.line, self.buf.line_len(r.b.line)) } else { Pos::new(r.a.line, self.first_nonblank(r.a.line)) };
                        self.start_insert(false);
                    }
                    _ => {
                        self.cur = if append { r.b } else { r.a };
                        self.start_insert(false);
                    }
                }
            }
        }
    }

    /// Текущее выделение как участок для оператора.
    pub(crate) fn visual_as_range(&self, force_line: bool) -> Option<Range> {
        let Mode::Visual(k) = self.mode else { return None };
        let (a, b) = if self.vstart <= self.cur { (self.vstart, self.cur) } else { (self.cur, self.vstart) };
        if force_line && k != VKind::Block {
            return Some(Range { a: Pos::new(a.line, 0), b: Pos::new(b.line, 0), kind: RKind::Line });
        }
        Some(match k {
            VKind::Line => Range { a: Pos::new(a.line, 0), b: Pos::new(b.line, 0), kind: RKind::Line },
            VKind::Char => {
                let len = self.buf.line_len(b.line);
                let end = if b.col < len {
                    Pos::new(b.line, b.col + 1)
                } else if b.line + 1 < self.buf.line_count() {
                    Pos::new(b.line + 1, 0)
                } else {
                    b
                };
                Range { a, b: end, kind: RKind::Char }
            }
            VKind::Block => {
                let xa = self.col_to_x(self.vstart.line, self.vstart.col);
                let xb = self.col_to_x(self.cur.line, self.cur.col);
                let (l, r) = if xa <= xb { (xa, xb) } else { (xb, xa) };
                let r = if self.want_x == Some(usize::MAX) { usize::MAX } else { r };
                Range { a, b, kind: RKind::Block(l, r) }
            }
        })
    }
}
