//! Команды командной строки (`:`).
//!
//! Строка разбирается как в vim: `[диапазон]команда[!] [аргументы]`.
//! Диапазон — адреса через запятую: `.`, `$`, `%`, число, `'x` (метка,
//! `'<,'>` — выделение), `/образец/`, `?образец?`, со смещениями `+N`/`-N`.
//! Несколько команд можно разделить `|`.

use std::path::PathBuf;

use tvision::{Ctx, Info};

use crate::buffer::Pos;
use crate::config::{find_option, MapMode, Mapping};
use crate::editor::{absolute, cmd, Mode, VimEditor};
use crate::keys::{format_keys, parse_keys, text_keys, VKey};
use crate::normal::{Op, RKind, Range};
use crate::regex::expand_replacement;

/// Команды: полное имя и минимальная длина сокращения.
const COMMANDS: &[(&str, usize)] = &[
    ("write", 1),
    ("wq", 2),
    ("wall", 2),
    ("wqall", 3),
    ("xit", 1),
    ("xall", 2),
    ("exit", 3),
    ("update", 2),
    ("quit", 1),
    ("qall", 2),
    ("quitall", 5),
    ("close", 3),
    ("edit", 1),
    ("enew", 3),
    ("new", 3),
    ("split", 2),
    ("vsplit", 2),
    ("vnew", 3),
    ("tabnew", 4),
    ("tabedit", 4),
    ("read", 1),
    ("substitute", 1),
    ("&", 1),
    ("&&", 2),
    ("global", 1),
    ("vglobal", 1),
    ("delete", 1),
    ("yank", 1),
    ("put", 2),
    ("move", 1),
    ("copy", 2),
    ("t", 1),
    ("join", 1),
    (">", 1),
    ("<", 1),
    ("normal", 4),
    ("sort", 3),
    ("set", 2),
    ("setlocal", 4),
    ("setglobal", 4),
    ("setfiletype", 4),
    ("syntax", 2),
    ("colorscheme", 4),
    ("map", 3),
    ("nmap", 2),
    ("imap", 2),
    ("vmap", 2),
    ("xmap", 2),
    ("noremap", 2),
    ("nnoremap", 2),
    ("inoremap", 3),
    ("vnoremap", 2),
    ("xnoremap", 2),
    ("unmap", 3),
    ("nunmap", 3),
    ("iunmap", 3),
    ("vunmap", 3),
    ("nohlsearch", 3),
    ("help", 1),
    ("options", 3),
    ("preferences", 4),
    ("marks", 4),
    ("registers", 3),
    ("display", 2),
    ("pwd", 2),
    ("cd", 2),
    ("bnext", 2),
    ("bprevious", 2),
    ("bNext", 2),
    ("retab", 3),
    ("filetype", 5),
    ("echo", 2),
    ("undo", 1),
    ("redo", 3),
    ("goto", 2),
    ("!", 1),
    ("fileencoding", 5),
];

/// Имена для дополнения по Tab.
pub const COMMAND_NAMES: &[&str] = &[
    "write",
    "wq",
    "wall",
    "wqall",
    "xit",
    "xall",
    "update",
    "quit",
    "qall",
    "close",
    "edit",
    "enew",
    "new",
    "split",
    "tabnew",
    "read",
    "substitute",
    "global",
    "vglobal",
    "delete",
    "yank",
    "put",
    "move",
    "copy",
    "join",
    "normal",
    "sort",
    "set",
    "setlocal",
    "setfiletype",
    "syntax",
    "colorscheme",
    "map",
    "nmap",
    "imap",
    "vmap",
    "noremap",
    "nnoremap",
    "inoremap",
    "vnoremap",
    "unmap",
    "nunmap",
    "iunmap",
    "vunmap",
    "nohlsearch",
    "help",
    "options",
    "preferences",
    "marks",
    "registers",
    "pwd",
    "cd",
    "bnext",
    "bprevious",
    "retab",
    "filetype",
    "echo",
    "undo",
    "redo",
];

fn resolve(name: &str) -> Option<&'static str> {
    if let Some((n, _)) = COMMANDS.iter().find(|(n, _)| *n == name) {
        return Some(n);
    }
    COMMANDS.iter().find(|(n, min)| n.starts_with(name) && name.len() >= *min).map(|(n, _)| *n)
}

/// Команды, в аргументах которых `|` — обычный символ.
fn takes_bar(name: &str) -> bool {
    matches!(
        name,
        "global"
            | "vglobal"
            | "normal"
            | "!"
            | "map"
            | "nmap"
            | "imap"
            | "vmap"
            | "xmap"
            | "noremap"
            | "nnoremap"
            | "inoremap"
            | "vnoremap"
            | "xnoremap"
            | "read"
    )
}

/// Разобранная строка команды.
struct ExLine {
    range: Option<(usize, usize)>,
    /// Диапазон задан двумя адресами (для `:N` — перейти к строке).
    name: String,
    bang: bool,
    arg: String,
}

impl VimEditor {
    /// Выполнить строку команды `:`.
    pub fn execute_ex(&mut self, line: &str, ctx: &mut Ctx) {
        let line = line.trim_start_matches([':', ' ', '\t']);
        if line.is_empty() {
            return;
        }
        match self.parse_ex(line) {
            Ok((ex, rest)) => {
                self.run_ex(ex, ctx);
                if let Some(rest) = rest {
                    self.execute_ex(&rest, ctx);
                }
            }
            Err(e) => self.error(&e),
        }
        self.commit();
        if !matches!(self.mode, Mode::Insert | Mode::Replace | Mode::Cmdline(_)) {
            self.cur = self.clamp_normal(self.cur);
        }
    }

    /// Номер строки по адресу.
    fn parse_address(&mut self, s: &[char], i: &mut usize) -> Result<Option<usize>, String> {
        let last = self.buf.line_count() - 1;
        while *i < s.len() && s[*i] == ' ' {
            *i += 1;
        }
        let mut base: Option<isize> = None;
        if *i < s.len() {
            match s[*i] {
                '.' => {
                    base = Some(self.cur.line as isize);
                    *i += 1;
                }
                '$' => {
                    base = Some(last as isize);
                    *i += 1;
                }
                c if c.is_ascii_digit() => {
                    let mut n = 0isize;
                    while *i < s.len() && s[*i].is_ascii_digit() {
                        n = n.saturating_mul(10) + s[*i].to_digit(10).unwrap() as isize;
                        *i += 1;
                    }
                    base = Some((n - 1).max(0));
                }
                '\'' => {
                    let m = *s.get(*i + 1).ok_or("E20: Метка не установлена")?;
                    *i += 2;
                    let p = match m {
                        '<' => self.last_visual.map(|(a, b, _)| a.min(b)),
                        '>' => self.last_visual.map(|(a, b, _)| a.max(b)),
                        '.' => self.last_change_pos,
                        '\'' => self.last_jump,
                        c => self.marks.get(&c).copied(),
                    };
                    base = Some(p.ok_or("E20: Метка не установлена")?.line as isize);
                }
                d @ ('/' | '?') => {
                    *i += 1;
                    let start = *i;
                    while *i < s.len() && s[*i] != d {
                        if s[*i] == '\\' {
                            *i += 1;
                        }
                        *i += 1;
                    }
                    let pat: String = s[start..(*i).min(s.len())].iter().collect();
                    if *i < s.len() {
                        *i += 1;
                    }
                    let pat = if pat.is_empty() {
                        self.search.as_ref().map(|x| x.0.clone()).ok_or("E35: Нет предыдущего образца")?
                    } else {
                        pat
                    };
                    let re = self.make_regex(&pat)?;
                    let from = if d == '/' { Pos::new(self.cur.line, self.buf.line_len(self.cur.line)) } else { Pos::new(self.cur.line, 0) };
                    let (p, _) = self.find_match(&re, from, d == '/', true).ok_or_else(|| format!("E486: Образец не найден: {}", pat))?;
                    base = Some(p.line as isize);
                }
                '+' | '-' => base = Some(self.cur.line as isize),
                _ => {}
            }
        }
        // Смещения.
        while *i < s.len() && (s[*i] == '+' || s[*i] == '-') {
            let sign = if s[*i] == '+' { 1 } else { -1 };
            *i += 1;
            let mut n = 0isize;
            let mut any = false;
            while *i < s.len() && s[*i].is_ascii_digit() {
                n = n * 10 + s[*i].to_digit(10).unwrap() as isize;
                *i += 1;
                any = true;
            }
            if !any {
                n = 1;
            }
            base = Some(base.unwrap_or(self.cur.line as isize) + sign * n);
        }
        match base {
            None => Ok(None),
            Some(b) if b < 0 || b as usize > last => Err("E16: Неверный диапазон".into()),
            Some(b) => Ok(Some(b as usize)),
        }
    }

    /// Разобрать строку; вторая часть — то, что после `|`.
    fn parse_ex(&mut self, line: &str) -> Result<(ExLine, Option<String>), String> {
        let s: Vec<char> = line.chars().collect();
        let mut i = 0;
        let last = self.buf.line_count() - 1;
        let mut range = None;
        while i < s.len() && s[i] == ' ' {
            i += 1;
        }
        if i < s.len() && s[i] == '%' {
            range = Some((0, last));
            i += 1;
        } else if let Some(a) = self.parse_address(&s, &mut i)? {
            let mut b = a;
            while i < s.len() && (s[i] == ',' || s[i] == ';') {
                if s[i] == ';' {
                    self.cur = Pos::new(b, 0);
                }
                i += 1;
                b = self.parse_address(&s, &mut i)?.unwrap_or(self.cur.line);
            }
            range = Some((a, b));
        } else if i < s.len() && s[i] == ',' {
            i += 1;
            let b = self.parse_address(&s, &mut i)?.unwrap_or(self.cur.line);
            range = Some((self.cur.line, b));
        }
        if let Some((a, b)) = range {
            if a > b {
                range = Some((b, a));
            }
        }
        while i < s.len() && s[i] == ' ' {
            i += 1;
        }
        // Имя команды: буквы или один особый символ.
        let start = i;
        if i < s.len() && s[i].is_ascii_alphabetic() {
            while i < s.len() && s[i].is_ascii_alphabetic() {
                i += 1;
            }
            // «s» сразу с разделителем: s/a/b/, s#a#b#.
        } else if i < s.len() && matches!(s[i], '&' | '<' | '>' | '!' | '=') {
            i += 1;
            if s[start] == '&' && i < s.len() && s[i] == '&' {
                i += 1;
            }
        }
        let raw: String = s[start..i].iter().collect();
        let name = if raw.is_empty() {
            String::new()
        } else {
            // Сокращения vim для «s», «g», «v» с прилепленными флагами: «sg» не нужно,
            // но «substitute» с разделителем-буквой не поддерживаем.
            match resolve(&raw) {
                Some(n) => n.to_string(),
                None => {
                    // «s» + буквы-флаги без разделителя (например, «sno») — неизвестная команда.
                    return Err(format!("E492: Не команда редактора: {}", line));
                }
            }
        };
        let mut bang = false;
        if i < s.len() && s[i] == '!' && name != "!" {
            bang = true;
            i += 1;
        }
        let mut arg: String = s[i..].iter().collect();
        let mut rest = None;
        if !takes_bar(&name) {
            // `|` разделяет команды (кроме экранированного `\|` и внутри :s).
            let ac: Vec<char> = arg.chars().collect();
            let mut k = 0;
            let mut in_pat = name == "substitute";
            let mut delims = 0;
            let delim = if in_pat { ac.iter().find(|c| !c.is_whitespace()).copied() } else { None };
            while k < ac.len() {
                if ac[k] == '\\' {
                    k += 2;
                    continue;
                }
                if in_pat && Some(ac[k]) == delim {
                    delims += 1;
                    if delims >= 3 {
                        in_pat = false;
                    }
                } else if ac[k] == '|' && !in_pat {
                    rest = Some(ac[k + 1..].iter().collect::<String>());
                    arg = ac[..k].iter().collect();
                    break;
                }
                k += 1;
            }
        }
        let arg = if name == "normal" { arg.strip_prefix(' ').unwrap_or(&arg).to_string() } else { arg.trim().to_string() };
        Ok((ExLine { range, name, bang, arg }, rest))
    }

    fn need_editable(&mut self) -> bool {
        if self.readonly {
            self.error("E21: Изменения запрещены: файл только для чтения");
            false
        } else {
            true
        }
    }

    fn run_ex(&mut self, ex: ExLine, ctx: &mut Ctx) {
        let cur_line = self.cur.line;
        let last = self.buf.line_count() - 1;
        let (ra, rb) = ex.range.unwrap_or((cur_line, cur_line));
        let whole = ex.range.unwrap_or((0, last));
        let arg = ex.arg.clone();
        match ex.name.as_str() {
            "" => {
                // «:N» — перейти к строке.
                if let Some((_, b)) = ex.range {
                    self.last_jump = Some(self.cur);
                    self.cur = Pos::new(b, self.first_nonblank(b));
                }
            }
            "write" | "update" => {
                if ex.name == "update" && !self.modified() {
                    return;
                }
                if let Some(cmdline) = arg.strip_prefix('!') {
                    // :w !cmd — отдать текст команде.
                    let text = self.buf.text();
                    let out = run_shell(cmdline, Some(&text));
                    self.show_box(&out, ctx);
                    return;
                }
                let path = if arg.is_empty() { None } else { Some(PathBuf::from(&arg)) };
                if let (Some(p), Some((a, b))) = (&path, ex.range) {
                    // Часть текста в другой файл.
                    let text: Vec<String> = (a..=b).map(|l| self.buf.line_string(l)).collect();
                    match std::fs::write(absolute(p), text.join("\n") + "\n") {
                        Ok(_) => self.info(&format!("\"{}\" {}L записано", p.display(), b - a + 1)),
                        Err(e) => self.error(&format!("E212: {}", e)),
                    }
                    return;
                }
                if path.is_none() && self.file.as_os_str().is_empty() {
                    self.save_as_dialog(ctx);
                    return;
                }
                if let Some(p) = &path {
                    if absolute(p).exists() && !ex.bang && absolute(p) != self.file {
                        self.error("E13: Файл существует (добавьте ! для перезаписи)");
                        return;
                    }
                }
                match self.write_file(path.as_deref()) {
                    Ok(m) => self.info(&m),
                    Err(e) => self.error(&e),
                }
            }
            "wq" | "xit" | "exit" => {
                let need = ex.name == "wq" || self.modified();
                if need {
                    if self.file.as_os_str().is_empty() && arg.is_empty() {
                        self.close_after_save = true;
                        self.save_as_dialog(ctx);
                        return;
                    }
                    let path = if arg.is_empty() { None } else { Some(PathBuf::from(&arg)) };
                    if let Err(e) = self.write_file(path.as_deref()) {
                        self.error(&e);
                        return;
                    }
                }
                self.force_close = true;
                ctx.post_command(tvision::cm::CLOSE, Info::None);
                ctx.post_command(cmd::QUIT_IF_EMPTY, Info::None);
            }
            "wall" => ctx.post_command(cmd::WRITE_ALL, Info::None),
            "wqall" | "xall" => ctx.post_command(cmd::WRITE_QUIT_ALL, Info::None),
            "quit" | "close" => {
                if self.modified() && !ex.bang && !self.readonly {
                    self.error("E37: Изменения не сохранены (добавьте ! для выхода без записи)");
                    return;
                }
                self.force_close = true;
                ctx.post_command(tvision::cm::CLOSE, Info::None);
                if ex.name == "quit" {
                    ctx.post_command(cmd::QUIT_IF_EMPTY, Info::None);
                }
            }
            "qall" | "quitall" => ctx.post_command(if ex.bang { cmd::QUIT_FORCE } else { cmd::QUIT_ALL }, Info::None),
            "edit" => {
                if self.modified() && !ex.bang {
                    self.error("E37: Изменения не сохранены (добавьте ! чтобы отбросить их)");
                    return;
                }
                if arg.is_empty() {
                    if self.file.as_os_str().is_empty() {
                        self.error("E32: Нет имени файла");
                        return;
                    }
                    let f = self.file.clone();
                    let line = self.cur.line;
                    if let Err(e) = self.open(&f) {
                        self.error(&e);
                    }
                    self.cur = self.clamp_normal(Pos::new(line, 0));
                } else if let Err(e) = self.open(&PathBuf::from(&arg)) {
                    self.error(&e);
                }
            }
            "enew" => {
                if self.modified() && !ex.bang {
                    self.error("E37: Изменения не сохранены (добавьте ! чтобы отбросить их)");
                    return;
                }
                self.buf.set_text("");
                self.file = PathBuf::new();
                self.cur = Pos::default();
                self.lang = None;
            }
            "new" | "split" | "vsplit" | "vnew" | "tabnew" | "tabedit" => {
                let path = if arg.is_empty() && matches!(ex.name.as_str(), "split" | "vsplit") { self.file.display().to_string() } else { arg };
                ctx.post_command(cmd::EDIT_FILE, Info::Str(path));
            }
            "read" => {
                if !self.need_editable() {
                    return;
                }
                let text = if let Some(c) = arg.strip_prefix('!') {
                    run_shell(c, None)
                } else {
                    let p = if arg.is_empty() { self.file.clone() } else { absolute(&PathBuf::from(&arg)) };
                    match std::fs::read(&p) {
                        Ok(b) => crate::encoding::decode(&b).text.replace("\r\n", "\n"),
                        Err(e) => {
                            self.error(&format!("E484: Не удалось открыть {}: {}", p.display(), e));
                            return;
                        }
                    }
                };
                let text = text.strip_suffix('\n').unwrap_or(&text).to_string();
                let at = ex.range.map(|r| r.1).unwrap_or(cur_line);
                let e = Pos::new(at, self.buf.line_len(at));
                self.buf.insert(e, &format!("\n{}", text));
                self.cur = Pos::new(at + 1, self.first_nonblank(at + 1));
            }
            "substitute" | "&" | "&&" => {
                if !self.need_editable() {
                    return;
                }
                self.substitute(ra, rb, &arg, ex.name != "substitute" || arg.is_empty(), ex.name == "&&");
            }
            "global" | "vglobal" => {
                if !self.need_editable() {
                    return;
                }
                let invert = ex.name == "vglobal" || ex.bang;
                self.global(whole.0, whole.1, &arg, invert, ctx);
            }
            "delete" | "yank" => {
                // :d [x] [N]
                let mut reg = None;
                let mut n = None;
                for tok in arg.split_whitespace() {
                    if let Ok(v) = tok.parse::<usize>() {
                        n = Some(v);
                    } else if tok.chars().count() == 1 {
                        reg = tok.chars().next();
                    }
                }
                let (a, b) = match n {
                    Some(n) => (rb, (rb + n - 1).min(last)),
                    None => (ra, rb),
                };
                let r = Range { a: Pos::new(a, 0), b: Pos::new(b, 0), kind: RKind::Line };
                if ex.name == "delete" {
                    if self.need_editable() {
                        self.apply(Op::Delete, r, reg, 1, ctx);
                    }
                } else {
                    let save = self.cur;
                    self.apply(Op::Yank, r, reg, 1, ctx);
                    self.cur = save;
                }
            }
            "put" => {
                if !self.need_editable() {
                    return;
                }
                let reg = arg.chars().next();
                let line = ex.range.map(|r| r.1).unwrap_or(cur_line);
                let Some(r) = self.get_reg(reg, ctx) else {
                    self.error("E353: Регистр пуст");
                    return;
                };
                let text = r.text.clone();
                if ex.bang {
                    self.buf.insert(Pos::new(line, 0), &format!("{}\n", text));
                    self.cur = Pos::new(line, 0);
                } else {
                    let e = Pos::new(line, self.buf.line_len(line));
                    self.buf.insert(e, &format!("\n{}", text));
                    self.cur = Pos::new(line + 1, 0);
                }
            }
            "move" | "copy" | "t" => {
                if !self.need_editable() {
                    return;
                }
                let s: Vec<char> = arg.chars().collect();
                let mut i = 0;
                let dest = if arg.trim() == "0" {
                    None
                } else {
                    match self.parse_address(&s, &mut i) {
                        Ok(Some(d)) => Some(d),
                        Ok(None) => {
                            self.error("E14: Неверный адрес");
                            return;
                        }
                        Err(e) => {
                            self.error(&e);
                            return;
                        }
                    }
                };
                let lines: Vec<String> = (ra..=rb).map(|l| self.buf.line_string(l)).collect();
                let text = lines.join("\n");
                let n = rb - ra + 1;
                if ex.name == "move" {
                    if let Some(d) = dest {
                        if d >= ra && d < rb {
                            self.error("E134: Нельзя переместить строки в самих себя");
                            return;
                        }
                    }
                    // Сначала вставляем, потом удаляем исходные (индексы с поправкой).
                    let ins_line = match dest {
                        None => 0,
                        Some(d) => d + 1,
                    };
                    self.insert_lines_at(ins_line, &text);
                    let (da, db) = if ins_line <= ra { (ra + n, rb + n) } else { (ra, rb) };
                    self.delete_lines(da, db);
                    let new_first = if ins_line <= ra { ins_line } else { ins_line - n };
                    self.cur = Pos::new(new_first + n - 1, 0);
                } else {
                    let ins_line = match dest {
                        None => 0,
                        Some(d) => d + 1,
                    };
                    self.insert_lines_at(ins_line, &text);
                    self.cur = Pos::new(ins_line + n - 1, 0);
                }
                self.cur.col = self.first_nonblank(self.cur.line);
            }
            "join" => {
                if !self.need_editable() {
                    return;
                }
                let (a, b) = if ex.range.map(|r| r.0 != r.1).unwrap_or(false) { (ra, rb) } else { (ra, (ra + 1).min(last)) };
                let n: usize = arg.trim().parse().unwrap_or(b - a + 1);
                self.join(a, n.max(2), !ex.bang);
            }
            ">" | "<" => {
                if !self.need_editable() {
                    return;
                }
                let times = 1 + arg.chars().filter(|&c| c == ex.name.chars().next().unwrap()).count();
                self.shift_lines(ra, rb, ex.name == ">", times);
                self.cur = Pos::new(rb, self.first_nonblank(rb));
            }
            "normal" => {
                if arg.is_empty() {
                    return;
                }
                let keys = text_keys(&arg);
                let lines: Vec<usize> = match ex.range {
                    Some((a, b)) => (a..=b).collect(),
                    None => vec![usize::MAX],
                };
                let mut offset: isize = 0;
                for l in lines {
                    if l != usize::MAX {
                        let ln = (l as isize + offset).max(0) as usize;
                        if ln >= self.buf.line_count() {
                            break;
                        }
                        self.cur = Pos::new(ln, 0);
                    }
                    let before = self.buf.line_count() as isize;
                    for &k in &keys {
                        if ex.bang {
                            self.process(k, ctx);
                        } else {
                            self.map_feed(k, ctx);
                        }
                    }
                    // Незаконченная команда/вставка завершается, как Esc.
                    self.pending.clear();
                    if matches!(self.mode, Mode::Insert | Mode::Replace) {
                        self.process(VKey::Esc, ctx);
                    }
                    if matches!(self.mode, Mode::Visual(_)) {
                        self.exit_visual();
                    }
                    if matches!(self.mode, Mode::Cmdline(_)) {
                        self.process(VKey::Esc, ctx);
                    }
                    offset += self.buf.line_count() as isize - before;
                }
            }
            "sort" => {
                if !self.need_editable() {
                    return;
                }
                let (a, b) = ex.range.unwrap_or((0, last));
                let mut lines: Vec<String> = (a..=b).map(|l| self.buf.line_string(l)).collect();
                let icase = arg.contains('i');
                let numeric = arg.contains('n');
                if numeric {
                    let num = |s: &str| -> i64 {
                        let t: String =
                            s.chars().skip_while(|c| !c.is_ascii_digit() && *c != '-').take_while(|c| c.is_ascii_digit() || *c == '-').collect();
                        t.parse().unwrap_or(i64::MIN)
                    };
                    lines.sort_by_key(|s| num(s));
                } else if icase {
                    lines.sort_by_key(|s| s.to_lowercase());
                } else {
                    lines.sort();
                }
                if ex.bang {
                    lines.reverse();
                }
                if arg.contains('u') {
                    lines.dedup_by(|x, y| if icase { x.to_lowercase() == y.to_lowercase() } else { x == y });
                }
                let e = Pos::new(b, self.buf.line_len(b));
                self.buf.replace(Pos::new(a, 0), e, &lines.join("\n"));
                self.cur = Pos::new(a, 0);
            }
            "set" | "setlocal" | "setglobal" => {
                if arg.is_empty() {
                    ctx.post_command(cmd::OPTIONS, Info::None);
                    return;
                }
                self.set_options(&arg, ctx);
            }
            "setfiletype" | "filetype" => {
                if arg.is_empty() {
                    if ex.name == "filetype" {
                        let ft = self.lang.map(|l| l.id).unwrap_or("text");
                        self.info(&format!("filetype={}", ft));
                    } else {
                        ctx.post_command(cmd::SET_FILETYPE, Info::None);
                    }
                    return;
                }
                if !self.set_lang(&arg) {
                    self.error(&format!("Неизвестный тип файла: {}", arg));
                }
            }
            "syntax" => match arg.as_str() {
                "" => ctx.post_command(cmd::SYNTAX, Info::None),
                "on" | "enable" => {
                    self.cfg.borrow_mut().options.syntax = true;
                    self.cfg.borrow_mut().touch();
                }
                "off" | "clear" => {
                    self.cfg.borrow_mut().options.syntax = false;
                    self.cfg.borrow_mut().touch();
                }
                other => self.error(&format!("E28: Неизвестная подкоманда: {}", other)),
            },
            "colorscheme" => {
                if arg.is_empty() {
                    ctx.post_command(cmd::COLORS, Info::None);
                    return;
                }
                if !self.cfg.borrow_mut().load_scheme(&arg) {
                    let names: Vec<&str> = crate::config::SCHEMES.iter().map(|s| s.name).collect();
                    self.error(&format!("E185: Нет схемы «{}» (есть: {})", arg, names.join(", ")));
                } else {
                    ctx.post_command(cmd::SAVE_CONFIG, Info::None);
                }
            }
            "map" | "nmap" | "imap" | "vmap" | "xmap" | "noremap" | "nnoremap" | "inoremap" | "vnoremap" | "xnoremap" => {
                let modes: Vec<MapMode> = match ex.name.chars().next().unwrap() {
                    'n' if ex.name != "noremap" => vec![MapMode::Normal],
                    'i' => vec![MapMode::Insert],
                    'v' | 'x' => vec![MapMode::Visual],
                    _ => vec![MapMode::Normal, MapMode::Visual],
                };
                let arg = arg.trim();
                if arg.is_empty() {
                    ctx.post_command(cmd::KEYMAP, Info::None);
                    return;
                }
                let (lhs, rhs) = match arg.split_once(char::is_whitespace) {
                    Some((l, r)) => (l.to_string(), r.trim_start().to_string()),
                    None => (arg.to_string(), String::new()),
                };
                let leader = self.cfg.borrow().options.leader;
                if let Err(e) = parse_keys(&lhs, leader).and_then(|_| parse_keys(&rhs, leader)) {
                    self.error(&e);
                    return;
                }
                if rhs.is_empty() {
                    let found: Vec<String> = self
                        .cfg
                        .borrow()
                        .mappings
                        .iter()
                        .filter(|m| modes.contains(&m.mode) && m.lhs == lhs)
                        .map(|m| format!("{}  {}  {}", m.mode.key(), m.lhs, m.rhs))
                        .collect();
                    if found.is_empty() {
                        self.error("Нет такого сопоставления");
                    } else {
                        self.info(&found.join(" | "));
                    }
                    return;
                }
                {
                    let mut c = self.cfg.borrow_mut();
                    for m in &modes {
                        c.mappings.retain(|x| !(x.mode == *m && x.lhs == lhs));
                        c.mappings.push(Mapping { mode: *m, lhs: lhs.clone(), rhs: rhs.clone() });
                    }
                    c.touch();
                }
                ctx.post_command(cmd::SAVE_CONFIG, Info::None);
            }
            "unmap" | "nunmap" | "iunmap" | "vunmap" => {
                let modes: Vec<MapMode> = match ex.name.chars().next().unwrap() {
                    'n' => vec![MapMode::Normal],
                    'i' => vec![MapMode::Insert],
                    'v' => vec![MapMode::Visual],
                    _ => vec![MapMode::Normal, MapMode::Visual],
                };
                let lhs = arg.trim().to_string();
                let before = self.cfg.borrow().mappings.len();
                self.cfg.borrow_mut().mappings.retain(|x| !(modes.contains(&x.mode) && x.lhs == lhs));
                if self.cfg.borrow().mappings.len() == before {
                    self.error("E31: Нет такого сопоставления");
                } else {
                    self.cfg.borrow_mut().touch();
                    ctx.post_command(cmd::SAVE_CONFIG, Info::None);
                }
            }
            "nohlsearch" => self.noh = true,
            "help" => ctx.post_command(cmd::HELP, Info::Str(arg)),
            "options" => ctx.post_command(cmd::OPTIONS, Info::None),
            "preferences" => ctx.post_command(cmd::APP_SETTINGS, Info::None),
            "marks" => {
                let mut v: Vec<(char, Pos)> = self.marks.iter().map(|(k, p)| (*k, *p)).collect();
                v.sort();
                let mut s = String::from("метка  строка  столб  текст\n");
                for (k, p) in v {
                    let t: String = self.buf.line_string(p.line.min(last)).trim().chars().take(30).collect();
                    s.push_str(&format!(" {}    {:>6} {:>6}  {}\n", k, p.line + 1, p.col, t));
                }
                self.show_box(s.trim_end(), ctx);
            }
            "registers" | "display" => {
                let mut v: Vec<(char, String, bool)> = self.registers.iter().map(|(k, r)| (*k, r.text.clone(), r.linewise)).collect();
                v.sort_by_key(|x| x.0);
                let mut s = String::from("рег  вид  содержимое\n");
                for (k, t, lw) in v {
                    let t: String = t.replace('\n', "^J").replace('\t', "^I").chars().take(40).collect();
                    s.push_str(&format!("\"{}   {}   {}\n", k, if lw { "l" } else { "c" }, t));
                }
                for (k, m) in &self.macros {
                    let t: String = format_keys(m).chars().take(40).collect();
                    s.push_str(&format!("@{}   m   {}\n", k, t));
                }
                self.show_box(s.trim_end(), ctx);
            }
            "pwd" => {
                let d = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default();
                self.info(&d);
            }
            "cd" => {
                let target = if arg.is_empty() {
                    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
                } else {
                    absolute(&PathBuf::from(&arg))
                };
                match std::env::set_current_dir(&target) {
                    Ok(_) => self.info(&target.display().to_string()),
                    Err(e) => self.error(&format!("E344: {}: {}", target.display(), e)),
                }
            }
            "bnext" => ctx.post_command(tvision::cm::NEXT, Info::None),
            "bprevious" | "bNext" => ctx.post_command(tvision::cm::PREV, Info::None),
            "retab" => {
                if !self.need_editable() {
                    return;
                }
                let (a, b) = ex.range.unwrap_or((0, last));
                if let Ok(n) = arg.trim().parse::<usize>() {
                    if n > 0 {
                        self.cfg.borrow_mut().options.tabstop = n;
                        self.cfg.borrow_mut().touch();
                    }
                }
                let (ts, _, et) = self.indent_params();
                for l in a..=b {
                    let fnb = self.first_nonblank(l);
                    if fnb == 0 {
                        continue;
                    }
                    let w = self.indent_width(l);
                    let ind = if et && !ex.bang {
                        " ".repeat(w)
                    } else {
                        let mut s = "\t".repeat(w / ts);
                        s.push_str(&" ".repeat(w % ts));
                        s
                    };
                    let old: String = self.buf.line(l)[..fnb].iter().collect();
                    if old != ind {
                        self.buf.replace(Pos::new(l, 0), Pos::new(l, fnb), &ind);
                    }
                }
            }
            "echo" => {
                let t = arg.trim_matches(|c| c == '"' || c == '\'');
                self.info(t);
            }
            "undo" => self.run_keys(&[VKey::Char('u')], ctx),
            "redo" => self.run_keys(&[VKey::Ctrl('r')], ctx),
            "goto" => {
                let n: usize = arg.trim().parse().unwrap_or(1);
                let text = self.buf.text();
                let mut off = 0usize;
                for (l, line) in text.split('\n').enumerate() {
                    let len = line.len() + 1;
                    if off + len > n.saturating_sub(1) {
                        let byte = n.saturating_sub(1) - off;
                        let col = line[..byte.min(line.len())].chars().count();
                        self.cur = Pos::new(l, col);
                        break;
                    }
                    off += len;
                }
            }
            "fileencoding" => {
                if arg.is_empty() {
                    self.info(&format!("fileencoding={}", self.encoding.name()));
                } else {
                    let v = arg.trim_start_matches('=');
                    match crate::encoding::Encoding::from_name(v) {
                        Some(e) => {
                            self.encoding = e;
                            self.buf.mark_modified();
                        }
                        None => self.error(&format!("E474: Неизвестная кодировка: {}", v)),
                    }
                }
            }
            "!" => {
                if let Some((a, b)) = ex.range {
                    // Фильтр: строки через внешнюю команду.
                    if !self.need_editable() {
                        return;
                    }
                    let input: Vec<String> = (a..=b).map(|l| self.buf.line_string(l)).collect();
                    let out = run_shell(&arg, Some(&(input.join("\n") + "\n")));
                    let out = out.strip_suffix('\n').unwrap_or(&out).to_string();
                    let e = Pos::new(b, self.buf.line_len(b));
                    self.buf.replace(Pos::new(a, 0), e, &out);
                    self.cur = Pos::new(a, 0);
                } else {
                    let out = run_shell(&arg, None);
                    self.show_box(if out.trim().is_empty() { "(нет вывода)" } else { out.trim_end() }, ctx);
                }
            }
            other => self.error(&format!("E492: Не команда редактора: {}", other)),
        }
    }

    /// Вставить строки перед строкой `at` (`at == line_count` — в конец).
    fn insert_lines_at(&mut self, at: usize, text: &str) {
        if at >= self.buf.line_count() {
            let last = self.buf.line_count() - 1;
            let e = Pos::new(last, self.buf.line_len(last));
            self.buf.insert(e, &format!("\n{}", text));
        } else {
            self.buf.insert(Pos::new(at, 0), &format!("{}\n", text));
        }
    }

    /// `:s/образец/замена/флаги`.
    fn substitute(&mut self, a: usize, b: usize, arg: &str, repeat: bool, keep_flags: bool) {
        let (pat, rep, flags) = if repeat && (arg.is_empty() || arg.chars().all(|c| "gicIne&".contains(c))) {
            match &self.last_sub {
                Some((p, r, f)) => {
                    let fl = if keep_flags || arg.starts_with('&') { format!("{}{}", f, arg.trim_start_matches('&')) } else { arg.to_string() };
                    (p.clone(), r.clone(), fl)
                }
                None => {
                    self.error("E35: Нет предыдущей замены");
                    return;
                }
            }
        } else {
            let ac: Vec<char> = arg.chars().collect();
            let Some(&d) = ac.first() else {
                self.error("E35: Нет образца");
                return;
            };
            if d.is_alphanumeric() || d == '\\' || d == '"' || d == '|' || d == ' ' {
                self.error("E146: Разделителем не может быть буква");
                return;
            }
            let mut parts: Vec<String> = vec![String::new()];
            let mut i = 1;
            while i < ac.len() {
                let c = ac[i];
                if c == '\\' && i + 1 < ac.len() {
                    if ac[i + 1] == d {
                        parts.last_mut().unwrap().push(d);
                    } else {
                        parts.last_mut().unwrap().push('\\');
                        parts.last_mut().unwrap().push(ac[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                if c == d && parts.len() < 3 {
                    parts.push(String::new());
                    i += 1;
                    continue;
                }
                parts.last_mut().unwrap().push(c);
                i += 1;
            }
            let pat = parts[0].clone();
            let rep = parts.get(1).cloned().unwrap_or_default();
            let flags = parts.get(2).cloned().unwrap_or_default();
            let pat = if pat.is_empty() {
                match &self.search {
                    Some((p, _)) => p.clone(),
                    None => {
                        self.error("E35: Нет предыдущего образца поиска");
                        return;
                    }
                }
            } else {
                pat
            };
            (pat, rep, flags)
        };
        let flags_clean: String = flags.chars().filter(|c| *c != '&').collect();
        self.last_sub = Some((pat.clone(), rep.clone(), flags_clean.clone()));
        self.search = Some((pat.clone(), true));
        self.noh = false;
        let global = flags.contains('g');
        let count_only = flags.contains('n');
        let silent = flags.contains('e');
        let re = {
            let (ic, sc) = {
                let o = &self.cfg.borrow().options;
                (o.ignorecase, o.smartcase)
            };
            let ic = if flags.contains('i') {
                true
            } else if flags.contains('I') {
                false
            } else {
                ic
            };
            match crate::regex::Regex::new(&pat, ic, sc && !flags.contains('i')) {
                Ok(r) => r,
                Err(e) => {
                    self.error(&e);
                    return;
                }
            }
        };
        let mut total = 0;
        let mut lines = 0;
        let mut last_line = None;
        // С конца, чтобы переводы строк в замене не сбивали номера.
        for l in (a..=b.min(self.buf.line_count() - 1)).rev() {
            let line = self.buf.line(l).to_vec();
            let mut out = String::new();
            let mut pos = 0;
            let mut n = 0;
            while pos <= line.len() {
                let Some((s, e, caps)) = re.find_at(&line, pos) else { break };
                out.extend(&line[pos..s]);
                out.push_str(&expand_replacement(&rep, &line, &caps));
                n += 1;
                if e > s {
                    pos = e;
                } else {
                    if s < line.len() {
                        out.push(line[s]);
                    }
                    pos = s + 1;
                }
                if !global {
                    break;
                }
            }
            if n == 0 {
                continue;
            }
            if pos < line.len() {
                out.extend(&line[pos..]);
            }
            total += n;
            lines += 1;
            last_line.get_or_insert(l);
            if !count_only {
                self.buf.set_line(l, &out);
            }
        }
        if total == 0 {
            if !silent {
                self.error(&format!("E486: Образец не найден: {}", pat));
            }
            return;
        }
        if let Some(l) = last_line {
            if !count_only {
                self.cur = Pos::new(l, self.first_nonblank(l));
            }
        }
        if count_only {
            self.info(&format!("совпадений: {} в строках: {}", total, lines));
        } else if total > 1 || lines > 1 {
            self.info(&format!("замен: {} в строках: {}", total, lines));
        }
    }

    /// `:g/образец/команда` (`invert` — для строк без совпадения).
    fn global(&mut self, a: usize, b: usize, arg: &str, invert: bool, ctx: &mut Ctx) {
        let ac: Vec<char> = arg.chars().collect();
        let Some(&d) = ac.first() else {
            self.error("E35: Нет образца");
            return;
        };
        let mut i = 1;
        let mut pat = String::new();
        while i < ac.len() && ac[i] != d {
            if ac[i] == '\\' && i + 1 < ac.len() {
                if ac[i + 1] != d {
                    pat.push('\\');
                }
                pat.push(ac[i + 1]);
                i += 2;
                continue;
            }
            pat.push(ac[i]);
            i += 1;
        }
        let command: String = ac.get(i + 1..).map(|s| s.iter().collect()).unwrap_or_default();
        let command = if command.trim().is_empty() { "p".to_string() } else { command };
        let pat = if pat.is_empty() { self.search.as_ref().map(|s| s.0.clone()).unwrap_or_default() } else { pat };
        let re = match self.make_regex(&pat) {
            Ok(r) => r,
            Err(e) => {
                self.error(&e);
                return;
            }
        };
        self.search = Some((pat, true));
        let targets: Vec<usize> = (a..=b).filter(|&l| re.find_at(self.buf.line(l), 0).is_some() != invert).collect();
        if targets.is_empty() {
            self.error("E486: Образец не найден");
            return;
        }
        if command.trim() == "p" || command.trim() == "print" {
            let s: Vec<String> = targets.iter().take(200).map(|&l| format!("{:>5} {}", l + 1, self.buf.line_string(l))).collect();
            self.show_box(&s.join("\n"), ctx);
            return;
        }
        // Номера строк сдвигаются по мере удаления/вставки.
        let mut shift: isize = 0;
        let mut prev: Option<usize> = None;
        for &l in &targets {
            let ln = l as isize + shift;
            if ln < 0 || ln as usize >= self.buf.line_count() {
                continue;
            }
            // Строка могла быть удалена предыдущей командой (например, :g/x/.,+1d).
            if let Some(p) = prev {
                if (ln as usize) < p {
                    continue;
                }
            }
            self.cur = Pos::new(ln as usize, 0);
            let before = self.buf.line_count() as isize;
            self.execute_ex(&command, ctx);
            let after = self.buf.line_count() as isize;
            shift += after - before;
            prev = Some(ln as usize);
            if matches!(self.message, Some((_, true))) && !command.trim_start().starts_with('s') {
                break;
            }
        }
    }

    /// `:set …`.
    fn set_options(&mut self, arg: &str, ctx: &mut Ctx) {
        let mut shown = Vec::new();
        for tok in arg.split_whitespace() {
            if tok == "all" {
                let mut o = self.cfg.borrow().options.clone();
                let s: Vec<String> = crate::config::OPTIONS.iter().map(|i| o.show(i.name).trim().to_string()).collect();
                self.show_box(&s.join("\n"), ctx);
                return;
            }
            let (name, op, val) = if let Some(p) = tok.find(['=', ':']) {
                let (n, v) = tok.split_at(p);
                let (n, op) = if let Some(n2) = n.strip_suffix('+') {
                    (n2, '+')
                } else if let Some(n2) = n.strip_suffix('-') {
                    (n2, '-')
                } else if let Some(n2) = n.strip_suffix('^') {
                    (n2, '^')
                } else {
                    (n, '=')
                };
                (n.to_string(), op, Some(v[1..].to_string()))
            } else {
                (tok.to_string(), ' ', None)
            };
            // Параметры буфера.
            match name.as_str() {
                "filetype" | "ft" | "syntax" if val.is_some() => {
                    let v = val.unwrap();
                    if !self.set_lang(&v) {
                        self.error(&format!("Неизвестный тип файла: {}", v));
                        return;
                    }
                    continue;
                }
                "filetype" | "ft" => {
                    shown.push(format!("filetype={}", self.lang.map(|l| l.id).unwrap_or("")));
                    continue;
                }
                "fileencoding" | "fenc" | "encoding" | "enc" => {
                    match val {
                        Some(v) => match crate::encoding::Encoding::from_name(&v) {
                            Some(e) => {
                                self.encoding = e;
                                self.buf.mark_modified();
                            }
                            None => {
                                self.error(&format!("E474: Неизвестная кодировка: {}", v));
                                return;
                            }
                        },
                        None => shown.push(format!("fileencoding={}", self.encoding.name())),
                    }
                    continue;
                }
                "fileformat" | "ff" => {
                    match val.as_deref() {
                        Some("dos") => {
                            self.crlf = true;
                            self.buf.mark_modified();
                        }
                        Some("unix") | Some("mac") => {
                            self.crlf = false;
                            self.buf.mark_modified();
                        }
                        Some(v) => {
                            self.error(&format!("E474: Неверный аргумент: {}", v));
                            return;
                        }
                        None => shown.push(format!("fileformat={}", if self.crlf { "dos" } else { "unix" })),
                    }
                    continue;
                }
                "readonly" | "ro" => {
                    self.readonly = true;
                    continue;
                }
                "noreadonly" | "noro" => {
                    self.readonly = false;
                    continue;
                }
                "clipboard" | "cb" if val.is_some() => {
                    let v = val.unwrap();
                    self.cfg.borrow_mut().options.clipboard = v.contains("unnamed");
                    self.cfg.borrow_mut().touch();
                    continue;
                }
                "mapleader" | "leader" if val.is_some() => {
                    if let Some(c) = val.unwrap().chars().next() {
                        self.cfg.borrow_mut().options.leader = c;
                    }
                    continue;
                }
                _ => {}
            }
            let (base, neg, inv, query) = if let Some(n) = name.strip_suffix('?') {
                (n.to_string(), false, false, true)
            } else if let Some(n) = name.strip_suffix('!') {
                (n.to_string(), false, true, false)
            } else if let Some(n) = name.strip_prefix("no").filter(|n| find_option(n).is_some()) {
                (n.to_string(), true, false, false)
            } else if let Some(n) = name.strip_prefix("inv").filter(|n| find_option(n).is_some()) {
                (n.to_string(), false, true, false)
            } else {
                (name.clone(), false, false, false)
            };
            let Some(info) = find_option(&base) else {
                self.error(&format!("E518: Неизвестный параметр: {}", name));
                return;
            };
            let mut c = self.cfg.borrow_mut();
            if query || (val.is_none() && !info.is_bool && !neg && !inv) {
                shown.push(c.options.show(info.name).trim().to_string());
                continue;
            }
            if info.is_bool {
                if val.is_some() {
                    drop(c);
                    self.error(&format!("E474: Неверный аргумент: {}", tok));
                    return;
                }
                let b = c.options.bool_mut(info.name).unwrap();
                *b = if inv { !*b } else { !neg };
            } else {
                let Some(Ok(v)) = val.map(|v| v.parse::<usize>()) else {
                    drop(c);
                    self.error(&format!("E521: После = нужно число: {}", tok));
                    return;
                };
                let n = c.options.num_mut(info.name).unwrap();
                *n = match op {
                    '+' => *n + v,
                    '-' => n.saturating_sub(v),
                    '^' => *n * v.max(1),
                    _ => v,
                };
                c.options.sanitize();
            }
            c.touch();
        }
        if !shown.is_empty() {
            self.info(&shown.join("  "));
        }
        self.last_sync = None;
        ctx.post_command(cmd::SAVE_CONFIG, Info::None);
    }
}

/// Выполнить команду оболочки и вернуть вывод (stdout + stderr).
pub fn run_shell(command: &str, input: Option<&str>) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    match c.spawn() {
        Ok(mut child) => {
            if let (Some(inp), Some(mut stdin)) = (input, child.stdin.take()) {
                let _ = stdin.write_all(inp.as_bytes());
            }
            match child.wait_with_output() {
                Ok(o) => {
                    let mut s = crate::encoding::decode(&o.stdout).text;
                    let e = crate::encoding::decode(&o.stderr).text;
                    if !e.is_empty() {
                        s.push_str(&e);
                    }
                    s.replace("\r\n", "\n")
                }
                Err(e) => format!("Ошибка: {}", e),
            }
        }
        Err(e) => format!("Не удалось запустить «{}»: {}", command, e),
    }
}
