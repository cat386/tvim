//! Текст редактора и отмена.
//!
//! Текст — массив строк из символов (`Vec<Vec<char>>`): так проще работать
//! с позициями «строка, символ» и с Unicode. Все изменения проходят через
//! две операции — [`Buffer::insert`] и [`Buffer::delete`]; каждая
//! записывается в текущую **группу отмены**. Группа закрывается
//! [`Buffer::commit`]: редактор делает это после каждой команды обычного
//! режима и при выходе из режима вставки, поэтому `u` отменяет команду
//! (`dw`, `3dd`, всю вставку `ihello<Esc>`) целиком — как в vim.

use std::cell::Cell;

/// Позиция в тексте: строка и номер символа в строке (с 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub const fn new(line: usize, col: usize) -> Self {
        Pos { line, col }
    }
}

#[derive(Clone, Debug)]
enum Change {
    Insert { at: Pos, text: String },
    Delete { at: Pos, text: String },
}

#[derive(Clone, Debug)]
struct Group {
    changes: Vec<Change>,
    id: u64,
}

/// Текст с историей изменений.
pub struct Buffer {
    pub lines: Vec<Vec<char>>,
    undo: Vec<Group>,
    redo: Vec<Group>,
    open: Vec<Change>,
    next_id: u64,
    saved_id: u64,
    /// Первая изменённая строка (для пересчёта подсветки); `usize::MAX` — нет.
    pub dirty: Cell<usize>,
    /// Счётчик изменений (растёт при любой правке).
    pub version: u64,
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer::new("")
    }
}

/// Разбить текст на строки (`\n`, `\r\n`, `\r`).
pub fn split_lines(s: &str) -> Vec<Vec<char>> {
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    s.split('\n').map(|l| l.chars().collect()).collect()
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Buffer {
            lines: split_lines(text),
            undo: Vec::new(),
            redo: Vec::new(),
            open: Vec::new(),
            next_id: 1,
            saved_id: 0,
            dirty: Cell::new(0),
            version: 0,
        }
    }

    /// Заменить весь текст: история очищается, текст считается сохранённым.
    pub fn set_text(&mut self, text: &str) {
        *self = Buffer::new(text);
    }

    /// Весь текст (строки через `\n`).
    pub fn text(&self) -> String {
        let mut s = String::new();
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.extend(l.iter());
        }
        s
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, i: usize) -> &[char] {
        self.lines.get(i).map(|l| l.as_slice()).unwrap_or(&[])
    }

    pub fn line_len(&self, i: usize) -> usize {
        self.lines.get(i).map(|l| l.len()).unwrap_or(0)
    }

    pub fn line_string(&self, i: usize) -> String {
        self.line(i).iter().collect()
    }

    /// Позиция, приведённая к существующей.
    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        Pos::new(line, p.col.min(self.lines[line].len()))
    }

    /// Конец текста.
    pub fn end(&self) -> Pos {
        let l = self.lines.len() - 1;
        Pos::new(l, self.lines[l].len())
    }

    /// Текст между позициями `[a, b)`.
    pub fn slice(&self, a: Pos, b: Pos) -> String {
        let a = self.clamp(a);
        let b = self.clamp(b);
        if b <= a {
            return String::new();
        }
        if a.line == b.line {
            return self.lines[a.line][a.col..b.col].iter().collect();
        }
        let mut s: String = self.lines[a.line][a.col..].iter().collect();
        for l in a.line + 1..b.line {
            s.push('\n');
            s.extend(self.lines[l].iter());
        }
        s.push('\n');
        s.extend(self.lines[b.line][..b.col].iter());
        s
    }

    fn touch(&mut self, line: usize) {
        if line < self.dirty.get() {
            self.dirty.set(line);
        }
        self.version += 1;
    }

    fn raw_insert(&mut self, at: Pos, text: &str) -> Pos {
        let at = self.clamp(at);
        self.touch(at.line);
        let parts = split_lines(text);
        let tail: Vec<char> = self.lines[at.line].split_off(at.col);
        let n = parts.len();
        let mut iter = parts.into_iter();
        self.lines[at.line].extend(iter.next().unwrap_or_default());
        let mut line = at.line;
        for p in iter {
            line += 1;
            self.lines.insert(line, p);
        }
        let col = self.lines[line].len();
        self.lines[line].extend(tail);
        if n == 1 {
            Pos::new(at.line, col)
        } else {
            Pos::new(line, col)
        }
    }

    fn raw_delete(&mut self, a: Pos, b: Pos) -> String {
        let a = self.clamp(a);
        let b = self.clamp(b);
        if b <= a {
            return String::new();
        }
        self.touch(a.line);
        let text = self.slice(a, b);
        let tail: Vec<char> = self.lines[b.line][b.col..].to_vec();
        self.lines[a.line].truncate(a.col);
        self.lines[a.line].extend(tail);
        if b.line > a.line {
            self.lines.drain(a.line + 1..=b.line);
        }
        text
    }

    /// Вставить текст; возвращает позицию за вставленным.
    pub fn insert(&mut self, at: Pos, text: &str) -> Pos {
        if text.is_empty() {
            return self.clamp(at);
        }
        let at = self.clamp(at);
        let end = self.raw_insert(at, text);
        // Подряд идущие вставки (набор текста) склеиваются в одну запись.
        if let Some(Change::Insert { at: pa, text: pt }) = self.open.last_mut() {
            let pend = end_of(*pa, pt);
            if pend == at {
                pt.push_str(text);
                self.redo.clear();
                return end;
            }
        }
        self.open.push(Change::Insert { at, text: text.to_string() });
        self.redo.clear();
        end
    }

    /// Удалить `[a, b)`; возвращает удалённый текст.
    pub fn delete(&mut self, a: Pos, b: Pos) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let a = self.clamp(a);
        let text = self.raw_delete(a, b);
        if !text.is_empty() {
            self.open.push(Change::Delete { at: a, text: text.clone() });
            self.redo.clear();
        }
        text
    }

    /// Заменить `[a, b)` текстом; возвращает конец вставки.
    pub fn replace(&mut self, a: Pos, b: Pos, text: &str) -> Pos {
        self.delete(a, b);
        self.insert(a, text)
    }

    /// Заменить строку целиком (без перевода строки).
    pub fn set_line(&mut self, i: usize, text: &str) {
        let len = self.line_len(i);
        self.replace(Pos::new(i, 0), Pos::new(i, len), text);
    }

    /// Закрыть группу отмены.
    pub fn commit(&mut self) {
        if self.open.is_empty() {
            return;
        }
        let changes = std::mem::take(&mut self.open);
        let id = self.next_id;
        self.next_id += 1;
        self.undo.push(Group { changes, id });
    }

    /// Есть ли незакрытые изменения.
    pub fn has_open(&self) -> bool {
        !self.open.is_empty()
    }

    /// Отменить группу; возвращает позицию, куда поставить курсор.
    pub fn undo(&mut self) -> Option<Pos> {
        self.commit();
        let g = self.undo.pop()?;
        let mut pos = None::<Pos>;
        for c in g.changes.iter().rev() {
            match c {
                Change::Insert { at, text } => {
                    let e = end_of(*at, text);
                    self.raw_delete(*at, e);
                    pos = Some(*at);
                }
                Change::Delete { at, text } => {
                    self.raw_insert(*at, text);
                    pos = Some(*at);
                }
            }
        }
        self.redo.push(g);
        pos
    }

    /// Повторить отменённое.
    pub fn redo(&mut self) -> Option<Pos> {
        self.commit();
        let g = self.redo.pop()?;
        let mut pos = None;
        for c in &g.changes {
            match c {
                Change::Insert { at, text } => {
                    self.raw_insert(*at, text);
                    pos = Some(*at);
                }
                Change::Delete { at, text } => {
                    let e = end_of(*at, text);
                    self.raw_delete(*at, e);
                    pos = Some(*at);
                }
            }
        }
        self.undo.push(g);
        pos
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || !self.open.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Число записей в истории (для сообщения после `u`).
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    fn current_id(&self) -> u64 {
        if !self.open.is_empty() {
            return u64::MAX;
        }
        self.undo.last().map(|g| g.id).unwrap_or(0)
    }

    /// Изменён ли текст после последнего сохранения.
    pub fn modified(&self) -> bool {
        self.current_id() != self.saved_id
    }

    /// Отметить текущее состояние как сохранённое.
    pub fn mark_saved(&mut self) {
        self.commit();
        self.saved_id = self.current_id();
    }

    /// Считать текст изменённым (например, после `:e!` с ошибкой).
    pub fn mark_modified(&mut self) {
        self.saved_id = u64::MAX - 1;
    }
}

/// Позиция за концом текста `text`, вставленного в `at`.
pub fn end_of(at: Pos, text: &str) -> Pos {
    let mut p = at;
    for c in text.chars() {
        if c == '\n' {
            p.line += 1;
            p.col = 0;
        } else {
            p.col += 1;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_and_undo() {
        let mut b = Buffer::new("hello\nworld");
        assert!(!b.modified());
        b.insert(Pos::new(0, 5), " there");
        b.insert(Pos::new(0, 11), "!");
        b.commit();
        assert_eq!(b.text(), "hello there!\nworld");
        assert!(b.modified());
        let d = b.delete(Pos::new(0, 5), Pos::new(1, 0));
        assert_eq!(d, " there!\n");
        b.commit();
        assert_eq!(b.text(), "helloworld");
        assert_eq!(b.undo(), Some(Pos::new(0, 5)));
        assert_eq!(b.text(), "hello there!\nworld");
        b.undo();
        assert_eq!(b.text(), "hello\nworld");
        assert!(!b.modified());
        b.redo();
        assert_eq!(b.text(), "hello there!\nworld");
        b.mark_saved();
        assert!(!b.modified());
        b.undo();
        assert!(b.modified());
    }

    #[test]
    fn multiline_insert() {
        let mut b = Buffer::new("ab");
        let e = b.insert(Pos::new(0, 1), "1\n2\n3");
        assert_eq!(e, Pos::new(2, 1));
        assert_eq!(b.text(), "a1\n2\n3b");
        assert_eq!(b.slice(Pos::new(0, 1), Pos::new(2, 1)), "1\n2\n3");
        b.commit();
        b.undo();
        assert_eq!(b.text(), "ab");
    }
}
