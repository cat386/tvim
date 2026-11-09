//! Представление редактора: режимы vim, отрисовка, события.
//!
//! [`VimEditor`] — обычное представление tvision ([`View`]), которое
//! вставляется во внутреннюю область окна ([`crate::window::VimWindow`]).
//! Оно занимает всю область: сверху текст с номерами строк, под ним строка
//! состояния (режим, файл, тип, позиция) и командная строка vim.
//!
//! Части редактора разнесены по файлам:
//! * здесь — данные, клавиши верхнего уровня, режим вставки, командная
//!   строка, поиск, регистры, отрисовка;
//! * [`crate::normal`] — обычный и визуальный режимы (команды, движения,
//!   операторы, текстовые объекты);
//! * [`crate::ex`] — команды `:`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tvision::view::default_handle_event;
use tvision::*;

use crate::buffer::{Buffer, Pos};
use crate::config::{Elem, MapMode, Shared};
use crate::encoding::{self, Encoding};
use crate::keys::VKey;
use crate::regex::Regex;
use crate::syntax::{highlight, lang_by_id, LState, Lang};

/// Команды, которые редактор посылает приложению (и получает из меню).
pub mod cmd {
    /// Диалог «Параметры редактора».
    pub const OPTIONS: u16 = 300;
    /// Диалог «Цвета».
    pub const COLORS: u16 = 301;
    /// Диалог «Типы файлов и подсветка».
    pub const SYNTAX: u16 = 302;
    /// Диалог «Сопоставления клавиш».
    pub const KEYMAP: u16 = 303;
    /// Общие настройки tvision (тема, язык, мышь).
    pub const APP_SETTINGS: u16 = 304;
    /// Окно справки.
    pub const HELP: u16 = 305;
    /// Открыть файл в новом окне: `Info::Str(путь)` (пустой — новый файл).
    pub const EDIT_FILE: u16 = 306;
    /// Закрыть программу, если окон не осталось (после `:q`).
    pub const QUIT_IF_EMPTY: u16 = 307;
    /// Выйти без вопросов (`:qa!`).
    pub const QUIT_FORCE: u16 = 308;
    /// Записать все (`:wa`).
    pub const WRITE_ALL: u16 = 309;
    /// Настройки изменены — сохранить файл настроек.
    pub const SAVE_CONFIG: u16 = 310;
    /// Перейти к строке (меню «Поиск → Перейти к строке»).
    pub const GOTO_LINE: u16 = 311;
    /// Диалог «Выбор типа файла» для текущего окна.
    pub const SET_FILETYPE: u16 = 312;
    /// Записать все и выйти (`:wqa`, `:xa`).
    pub const WRITE_QUIT_ALL: u16 = 313;
    /// Выйти: изменённые — с вопросом (`:qa`).
    pub const QUIT_ALL: u16 = 314;
}

/// Метки модальных окон, открываемых самим редактором (`exec_result`).
mod tag {
    pub const SAVE_AS: u16 = 1;
    pub const SAVE_MODIFY: u16 = 2;
    pub const INFO: u16 = 3;
}

/// Вид визуального режима.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VKind {
    Char,
    Line,
    Block,
}

/// Вид командной строки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmdKind {
    Ex,
    SearchFwd,
    SearchBack,
}

/// Режим редактора.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Replace,
    Visual(VKind),
    Cmdline(CmdKind),
}

/// Содержимое регистра.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Register {
    pub text: String,
    /// Построчный (`yy`, `dd`, `V…y`).
    pub linewise: bool,
}

/// Запись для повтора командой `.`.
#[derive(Clone, Debug, Default)]
pub struct DotRec {
    pub reg: Option<char>,
    pub count: Option<usize>,
    /// Клавиши команды без регистра и счётчика.
    pub body: Vec<VKey>,
    /// Клавиши, набранные в режиме вставки (вместе с завершающим Esc).
    pub insert: Vec<VKey>,
}

/// Кэш подсветки: состояние на начале каждой строки.
#[derive(Default)]
pub(crate) struct SynCache {
    pub states: Vec<LState>,
    /// Сколько первых состояний достоверны.
    pub valid: usize,
    pub generation: u64,
    pub lang: &'static str,
}

/// Вставка в блок (`Ctrl-V … I/A`): строки, экранный столбец, текст
/// набирается в первой строке и повторяется в остальных при выходе.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlockInsert {
    pub first: usize,
    pub last: usize,
    pub x: usize,
    pub start: Pos,
    /// `A` (дописать): короткие строки дополняются пробелами; у `I` — пропускаются.
    pub append: bool,
}

/// Автодополнение слов (`Ctrl-N`/`Ctrl-P` в режиме вставки).
#[derive(Clone, Debug)]
pub(crate) struct Completion {
    pub start: Pos,
    pub prefix: String,
    pub items: Vec<String>,
    pub index: Option<usize>,
}

/// Редактор.
pub struct VimEditor {
    pub vs: ViewState,
    pub cfg: Shared,
    pub buf: Buffer,
    /// Курсор.
    pub cur: Pos,
    /// Прокрутка: x — экранный столбец, y — первая видимая строка.
    pub delta: Point,
    /// Желаемый экранный столбец для j/k (`usize::MAX` — конец строки после `$`).
    pub(crate) want_x: Option<usize>,
    pub mode: Mode,
    /// Режим до входа в командную строку (для `:'<,'>`).
    pub(crate) prev_mode: Mode,
    /// Набираемая команда обычного/визуального режима.
    pub(crate) pending: Vec<VKey>,
    /// Клавиши, ждущие продолжения сопоставления.
    pub(crate) map_pending: Vec<VKey>,
    /// Начало визуального выделения.
    pub(crate) vstart: Pos,
    /// Последнее выделение (для `gv` и `'<`, `'>`).
    pub(crate) last_visual: Option<(Pos, Pos, VKind)>,
    pub(crate) registers: HashMap<char, Register>,
    pub(crate) macros: HashMap<char, Vec<VKey>>,
    pub(crate) recording: Option<(char, Vec<VKey>)>,
    pub(crate) last_macro: Option<char>,
    pub(crate) last_change: Option<DotRec>,
    pub(crate) dot_pending: Option<DotRec>,
    pub(crate) replay_depth: usize,
    /// Повтор вставки: `3ihi<Esc>` вставит «hi» трижды.
    pub(crate) insert_repeat: usize,
    pub(crate) insert_open_line: Option<bool>,
    pub(crate) insert_typed: String,
    pub(crate) block_insert: Option<BlockInsert>,
    /// Ctrl-O в режиме вставки: одна команда обычного режима.
    pub(crate) one_shot: bool,
    /// Ctrl-R в режиме вставки или командной строке: ждём имя регистра.
    pub(crate) await_reg: bool,
    /// Символы, заменённые в режиме замены (для Backspace).
    pub(crate) replaced: Vec<Option<char>>,
    pub(crate) completion: Option<Completion>,
    /// Файл.
    pub file: PathBuf,
    pub encoding: Encoding,
    pub crlf: bool,
    /// Язык подсветки (`None` — обычный текст).
    pub lang: Option<&'static Lang>,
    pub readonly: bool,
    /// Имя для заголовка вместо имени файла (окно справки).
    pub display_name: Option<String>,
    pub(crate) syn: RefCell<SynCache>,
    /// Последний поиск: образец и направление (вперёд).
    pub(crate) search: Option<(String, bool)>,
    /// Подсветка найденного выключена `:noh` до следующего поиска.
    pub(crate) noh: bool,
    pub(crate) last_sub: Option<(String, String, String)>,
    /// Командная строка.
    pub(crate) cmdline: Vec<char>,
    pub(crate) cmd_pos: usize,
    pub(crate) cmd_history: Vec<String>,
    pub(crate) search_history: Vec<String>,
    pub(crate) hist_index: Option<usize>,
    pub(crate) hist_prefix: String,
    /// Где был курсор до начала поиска (incsearch, Esc возвращает).
    pub(crate) search_origin: Option<(Pos, Point)>,
    pub(crate) complete_state: Option<(String, Vec<String>, usize)>,
    /// Сообщение в командной строке: (текст, ошибка).
    pub message: Option<(String, bool)>,
    /// Последний поиск символа f/t: символ, вперёд, «до».
    pub(crate) last_find: Option<(char, bool, bool)>,
    pub(crate) marks: HashMap<char, Pos>,
    pub(crate) last_jump: Option<Pos>,
    pub(crate) last_insert: Option<Pos>,
    pub(crate) last_change_pos: Option<Pos>,
    /// Связанная вертикальная полоса прокрутки.
    pub v_scroll: Option<ViewId>,
    pub(crate) last_sync: Option<(i32, i32, i32)>,
    /// Команда, которую нужно повторить после ответа «Сохранить?».
    pub(crate) pending_close: Option<u16>,
    /// После записи (через «Сохранить как») — закрыть окно.
    pub(crate) close_after_save: bool,
    /// Разрешить закрыть изменённый файл (`:q!`).
    pub(crate) force_close: bool,
    /// Перетаскивание мышью: позиция нажатия.
    pub(crate) drag_from: Option<Pos>,
    /// Поколение настроек, которое видел редактор.
    pub(crate) last_cfg_gen: u64,
    /// Счётчик для поиска с числом (`3/foo`).
    pub(crate) pending_search_count: Option<usize>,
    /// Оператор, ждущий поиска (`d/foo<CR>`): оператор и регистр.
    pub(crate) op_after_search: Option<(crate::normal::Op, Option<char>)>,
}

/// Высота служебных строк внизу (строка состояния и командная строка).
pub const BOTTOM_ROWS: i32 = 2;

impl VimEditor {
    pub fn new(bounds: Rect, cfg: Shared, v_scroll: Option<ViewId>) -> Self {
        let mut vs = ViewState::new(bounds);
        vs.options |= of::SELECTABLE | of::FIRST_CLICK;
        vs.grow_mode = gf::GROW_HI_X | gf::GROW_HI_Y;
        vs.show_cursor();
        vs.block_cursor();
        VimEditor {
            vs,
            cfg,
            buf: Buffer::new(""),
            cur: Pos::default(),
            delta: Point::default(),
            want_x: None,
            mode: Mode::Normal,
            prev_mode: Mode::Normal,
            pending: Vec::new(),
            map_pending: Vec::new(),
            vstart: Pos::default(),
            last_visual: None,
            registers: HashMap::new(),
            macros: HashMap::new(),
            recording: None,
            last_macro: None,
            last_change: None,
            dot_pending: None,
            replay_depth: 0,
            insert_repeat: 1,
            insert_open_line: None,
            insert_typed: String::new(),
            block_insert: None,
            one_shot: false,
            await_reg: false,
            replaced: Vec::new(),
            completion: None,
            file: PathBuf::new(),
            encoding: Encoding::Utf8,
            crlf: cfg!(windows),
            lang: None,
            readonly: false,
            display_name: None,
            syn: RefCell::new(SynCache::default()),
            search: None,
            noh: false,
            last_sub: None,
            cmdline: Vec::new(),
            cmd_pos: 0,
            cmd_history: Vec::new(),
            search_history: Vec::new(),
            hist_index: None,
            hist_prefix: String::new(),
            search_origin: None,
            complete_state: None,
            message: None,
            last_find: None,
            marks: HashMap::new(),
            last_jump: None,
            last_insert: None,
            last_change_pos: None,
            v_scroll,
            last_sync: None,
            pending_close: None,
            close_after_save: false,
            force_close: false,
            drag_from: None,
            last_cfg_gen: 0,
            pending_search_count: None,
            op_after_search: None,
        }
    }

    // ------------------------------------------------------------------
    // Файл
    // ------------------------------------------------------------------

    /// Имя для заголовка и строки состояния.
    pub fn title(&self) -> String {
        if let Some(n) = &self.display_name {
            return n.clone();
        }
        if self.file.as_os_str().is_empty() {
            "[Без имени]".into()
        } else {
            self.file.display().to_string()
        }
    }

    /// Короткое имя файла.
    pub fn short_name(&self) -> String {
        if let Some(n) = &self.display_name {
            return n.clone();
        }
        if self.file.as_os_str().is_empty() {
            "[Без имени]".into()
        } else {
            self.file.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| self.file.display().to_string())
        }
    }

    /// Определить язык по имени файла и первой строке.
    pub fn detect_lang(&mut self) {
        let first = self.buf.line_string(0);
        let id = self.cfg.borrow().detect_lang(&self.file, &first);
        self.lang = id.and_then(|i| lang_by_id(&i));
        self.syn.borrow_mut().valid = 0;
    }

    /// Задать язык по идентификатору (`:set ft=rust`); пустой — без подсветки.
    pub fn set_lang(&mut self, id: &str) -> bool {
        if id.is_empty() || id == "text" || id == "none" {
            self.lang = None;
            return true;
        }
        match lang_by_id(id) {
            Some(l) => {
                self.lang = Some(l);
                self.syn.borrow_mut().valid = 0;
                true
            }
            None => false,
        }
    }

    /// Открыть файл (если его нет — новый буфер с этим именем).
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let path = absolute(path);
        let res = if path.exists() {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let d = encoding::decode(&bytes);
                    let mut text = d.text;
                    // Последний перевод строки — конец последней строки, а не новая строка.
                    if text.ends_with('\n') {
                        text.pop();
                        if text.ends_with('\r') {
                            text.pop();
                        }
                    }
                    self.buf.set_text(&text);
                    self.encoding = d.encoding;
                    self.crlf = d.crlf;
                    let lines = self.buf.line_count();
                    let msg = format!(
                        "\"{}\" {}L, {}B{}",
                        path.display(),
                        lines,
                        bytes.len(),
                        if d.encoding == Encoding::Cp1251 { " [cp1251]" } else { "" }
                    );
                    self.info(&msg);
                    Ok(())
                }
                Err(e) => Err(format!("Ошибка чтения {}: {}", path.display(), e)),
            }
        } else {
            self.buf.set_text("");
            self.info(&format!("\"{}\" [Новый файл]", path.display()));
            Ok(())
        };
        self.file = path;
        self.cur = Pos::default();
        self.delta = Point::default();
        self.marks.clear();
        self.readonly = false;
        self.detect_lang();
        res
    }

    /// Записать текст в файл (`None` — в свой).
    pub fn write_file(&mut self, path: Option<&Path>) -> Result<String, String> {
        let path = match path {
            Some(p) => absolute(p),
            None => {
                if self.file.as_os_str().is_empty() {
                    return Err("E32: Нет имени файла".into());
                }
                self.file.clone()
            }
        };
        let (backup, trim, fixeol) = {
            let c = self.cfg.borrow();
            (c.options.backup, c.options.trimtrailing, c.options.fixeol)
        };
        if trim {
            self.trim_trailing();
        }
        let mut text = self.buf.text();
        if fixeol && !(self.buf.line_count() == 1 && self.buf.line_len(0) == 0) {
            text.push('\n');
        }
        let bytes = encoding::encode(&text, self.encoding, self.crlf);
        if backup && path.exists() {
            let mut bak = path.clone().into_os_string();
            bak.push(".bak");
            let _ = std::fs::copy(&path, PathBuf::from(bak));
        }
        std::fs::write(&path, &bytes).map_err(|e| format!("E212: Не удалось записать {}: {}", path.display(), e))?;
        let same = path == self.file;
        if same || self.file.as_os_str().is_empty() {
            if self.file.as_os_str().is_empty() {
                self.file = path.clone();
                self.detect_lang();
            }
            self.buf.mark_saved();
        }
        Ok(format!("\"{}\" {}L, {}B записано", path.display(), self.buf.line_count(), bytes.len()))
    }

    /// Убрать пробелы в конце строк (одной записью отмены).
    pub fn trim_trailing(&mut self) {
        for i in 0..self.buf.line_count() {
            let l = self.buf.line(i);
            let keep = l.iter().rposition(|c| !c.is_whitespace()).map(|p| p + 1).unwrap_or(0);
            if keep < l.len() {
                let len = l.len();
                self.buf.delete(Pos::new(i, keep), Pos::new(i, len));
            }
        }
        self.buf.commit();
        self.cur = self.clamp_normal(self.cur);
    }

    pub fn modified(&self) -> bool {
        self.buf.modified()
    }

    // ------------------------------------------------------------------
    // Сообщения
    // ------------------------------------------------------------------

    pub fn info(&mut self, s: &str) {
        self.message = Some((s.to_string(), false));
    }

    pub fn error(&mut self, s: &str) {
        self.message = Some((s.to_string(), true));
    }

    // ------------------------------------------------------------------
    // Геометрия текста
    // ------------------------------------------------------------------

    /// Ширина табуляции, отступа и expandtab для этого буфера.
    pub fn indent_params(&self) -> (usize, usize, bool) {
        self.cfg.borrow().indent_for(self.lang.map(|l| l.id))
    }

    pub fn tabstop(&self) -> usize {
        self.indent_params().0.max(1)
    }

    /// Экранный столбец символа `col` строки.
    pub fn col_to_x(&self, line: usize, col: usize) -> usize {
        let ts = self.tabstop();
        let mut x = 0;
        for &c in self.buf.line(line).iter().take(col) {
            x += char_cells(c, x, ts);
        }
        x
    }

    /// Символ строки, на котором находится экранный столбец `x`.
    pub fn x_to_col(&self, line: usize, x: usize) -> usize {
        let ts = self.tabstop();
        let mut cx = 0;
        for (i, &c) in self.buf.line(line).iter().enumerate() {
            let w = char_cells(c, cx, ts);
            if cx + w > x {
                return i;
            }
            cx += w;
        }
        self.buf.line_len(line)
    }

    /// Первый непробельный символ строки.
    pub fn first_nonblank(&self, line: usize) -> usize {
        let l = self.buf.line(line);
        l.iter().position(|c| !c.is_whitespace()).unwrap_or(l.len())
    }

    /// Ведущие пробелы/табуляции строки.
    pub fn indent_of(&self, line: usize) -> String {
        self.buf.line(line).iter().take_while(|c| **c == ' ' || **c == '\t').collect()
    }

    /// Ширина отступа строки в столбцах.
    pub fn indent_width(&self, line: usize) -> usize {
        let fnb = self.first_nonblank(line);
        self.col_to_x(line, fnb)
    }

    /// Строка отступа заданной ширины (пробелы или табуляции).
    pub fn make_indent(&self, width: usize) -> String {
        let (ts, _, et) = self.indent_params();
        if et {
            " ".repeat(width)
        } else {
            let mut s = "\t".repeat(width / ts);
            s.push_str(&" ".repeat(width % ts));
            s
        }
    }

    /// Курсор обычного режима не стоит за концом строки.
    pub fn clamp_normal(&self, p: Pos) -> Pos {
        let p = self.buf.clamp(p);
        let len = self.buf.line_len(p.line);
        Pos::new(p.line, p.col.min(len.saturating_sub(1)))
    }

    /// Высота области текста.
    pub fn text_height(&self) -> i32 {
        (self.vs.size.y - BOTTOM_ROWS).max(1)
    }

    /// Ширина колонки номеров строк.
    pub fn gutter_width(&self) -> usize {
        let o = &self.cfg.borrow().options;
        if o.number || o.relativenumber {
            let digits = self.buf.line_count().to_string().len();
            digits.max(3) + 1
        } else {
            0
        }
    }

    /// Прокрутить так, чтобы курсор был виден (с учётом scrolloff).
    pub fn track_cursor(&mut self) {
        let h = self.text_height();
        let so = (self.cfg.borrow().options.scrolloff as i32).min((h - 1) / 2);
        let y = self.cur.line as i32;
        if y < self.delta.y + so {
            self.delta.y = (y - so).max(0);
        }
        if y > self.delta.y + h - 1 - so {
            self.delta.y = y - h + 1 + so;
        }
        let max_y = (self.buf.line_count() as i32 - 1).max(0);
        self.delta.y = self.delta.y.clamp(0, max_y);
        // По горизонтали.
        let tw = (self.vs.size.x - self.gutter_width() as i32).max(1);
        let x = self.col_to_x(self.cur.line, self.cur.col) as i32;
        if x < self.delta.x {
            self.delta.x = x;
        }
        if x >= self.delta.x + tw {
            self.delta.x = x - tw + 1;
        }
    }

    /// Курсор в пределах видимой области (после прокрутки).
    pub fn keep_cursor_in_view(&mut self) {
        let h = self.text_height();
        let so = (self.cfg.borrow().options.scrolloff as i32).min((h - 1) / 2);
        let top = self.delta.y + if self.delta.y > 0 { so } else { 0 };
        let last = self.buf.line_count() as i32 - 1;
        let bottom = (self.delta.y + h - 1 - if self.delta.y + h - 1 < last { so } else { 0 }).min(last);
        let y = (self.cur.line as i32).clamp(top.min(bottom), bottom.max(0));
        if y != self.cur.line as i32 {
            self.cur.line = y as usize;
            self.cur.col = self.first_nonblank(self.cur.line);
            self.cur = self.clamp_normal(self.cur);
        }
    }

    // ------------------------------------------------------------------
    // Регистры
    // ------------------------------------------------------------------

    /// Записать в регистр. `yank` — копирование (регистр 0), иначе удаление (1/-).
    pub fn set_reg(&mut self, reg: Option<char>, text: String, linewise: bool, yank: bool, ctx: &mut Ctx) {
        let r = Register { text, linewise };
        let clip = self.cfg.borrow().options.clipboard;
        match reg {
            Some('_') => return,
            Some(c @ 'a'..='z') => {
                self.registers.insert(c, r.clone());
            }
            Some(c @ 'A'..='Z') => {
                let lc = c.to_ascii_lowercase();
                let e = self.registers.entry(lc).or_default();
                if e.linewise || r.linewise {
                    if !e.text.is_empty() {
                        e.text.push('\n');
                    }
                    e.linewise = true;
                }
                e.text.push_str(&r.text);
                let merged = e.clone();
                self.registers.insert('"', merged);
                return;
            }
            Some('+') | Some('*') => {
                ctx.clipboard.set(&clip_text(&r));
            }
            _ => {
                if yank {
                    self.registers.insert('0', r.clone());
                } else if r.linewise || r.text.contains('\n') {
                    self.registers.insert('1', r.clone());
                } else {
                    self.registers.insert('-', r.clone());
                }
                if clip {
                    ctx.clipboard.set(&clip_text(&r));
                }
            }
        }
        self.registers.insert('"', r);
    }

    /// Прочитать регистр.
    pub fn get_reg(&mut self, reg: Option<char>, ctx: &mut Ctx) -> Option<Register> {
        let clip = self.cfg.borrow().options.clipboard;
        match reg {
            None | Some('"') => {
                let un = self.registers.get(&'"').cloned();
                if clip {
                    let sys = ctx.clipboard.get();
                    if !sys.is_empty() {
                        if let Some(u) = &un {
                            if clip_text(u) == sys || u.text == sys {
                                return un;
                            }
                        }
                        return Some(from_clip(&sys));
                    }
                }
                un
            }
            Some('+') | Some('*') => {
                let sys = ctx.clipboard.get();
                (!sys.is_empty()).then(|| from_clip(&sys))
            }
            Some('%') => Some(Register { text: self.title(), linewise: false }),
            Some('/') => self.search.as_ref().map(|(s, _)| Register { text: s.clone(), linewise: false }),
            Some(':') => self.cmd_history.last().map(|s| Register { text: s.clone(), linewise: false }),
            Some('.') => self.registers.get(&'.').cloned(),
            Some(c) => self.registers.get(&c.to_ascii_lowercase()).cloned(),
        }
    }

    // ------------------------------------------------------------------
    // Подсветка
    // ------------------------------------------------------------------

    /// Цвета символов строки `li` (пусто — без подсветки).
    pub(crate) fn syntax_line(&self, li: usize) -> Vec<Elem> {
        let (enabled, gen) = {
            let c = self.cfg.borrow();
            (c.options.syntax, c.generation)
        };
        let Some(lang) = self.lang.filter(|_| enabled) else { return Vec::new() };
        let mut syn = self.syn.borrow_mut();
        let dirty = self.buf.dirty.replace(usize::MAX);
        if syn.lang != lang.id || syn.generation != gen {
            syn.lang = lang.id;
            syn.generation = gen;
            syn.valid = 0;
        }
        if dirty != usize::MAX {
            syn.valid = syn.valid.min(dirty + 1);
        }
        let n = self.buf.line_count();
        if syn.states.len() < n + 1 {
            syn.states.resize(n + 1, LState::Normal);
        }
        if syn.valid == 0 {
            syn.states[0] = LState::Normal;
            syn.valid = 1;
        }
        let mut out = Vec::new();
        while syn.valid <= li.min(n) {
            let k = syn.valid;
            let st = syn.states[k - 1];
            let next = highlight(lang, self.buf.line(k - 1), st, &mut out);
            syn.states[k] = next;
            syn.valid += 1;
        }
        let st = syn.states[li.min(n)];
        highlight(lang, self.buf.line(li), st, &mut out);
        out
    }

    // ------------------------------------------------------------------
    // Ввод
    // ------------------------------------------------------------------

    /// Клавиша от пользователя (с записью макроса и сопоставлениями).
    pub fn key(&mut self, k: VKey, ctx: &mut Ctx) {
        if let Some((_, rec)) = &mut self.recording {
            rec.push(k);
        }
        self.map_feed(k, ctx);
        self.after_key(ctx);
    }

    /// Последовательность клавиш (для тестов и `:normal`).
    pub fn keys(&mut self, keys: &[VKey], ctx: &mut Ctx) {
        for &k in keys {
            self.key(k, ctx);
        }
    }

    fn map_mode(&self) -> Option<MapMode> {
        if !self.pending.is_empty() || self.await_reg {
            return None;
        }
        match self.mode {
            Mode::Normal => Some(MapMode::Normal),
            Mode::Visual(_) => Some(MapMode::Visual),
            Mode::Insert | Mode::Replace => Some(MapMode::Insert),
            Mode::Cmdline(_) => None,
        }
    }

    /// Применить сопоставления клавиш (без рекурсии, как `noremap`).
    pub(crate) fn map_feed(&mut self, k: VKey, ctx: &mut Ctx) {
        let Some(mode) = self.map_mode() else {
            self.process(k, ctx);
            return;
        };
        let maps = self.cfg.borrow().compiled_mappings(mode);
        if maps.is_empty() {
            self.process(k, ctx);
            return;
        }
        self.map_pending.push(k);
        let mp = self.map_pending.clone();
        let longer = maps.iter().any(|(l, _)| l.len() > mp.len() && l.starts_with(&mp));
        if longer {
            return;
        }
        if let Some((_, rhs)) = maps.iter().find(|(l, _)| *l == mp) {
            self.map_pending.clear();
            for k in rhs.clone() {
                self.process(k, ctx);
            }
            return;
        }
        // Совпадения нет: первая клавиша — как есть, остальные проверяем снова.
        let keys = std::mem::take(&mut self.map_pending);
        self.process(keys[0], ctx);
        for k in keys.into_iter().skip(1) {
            self.map_feed(k, ctx);
        }
    }

    /// Обработать клавишу в текущем режиме.
    pub(crate) fn process(&mut self, k: VKey, ctx: &mut Ctx) {
        match self.mode {
            Mode::Normal | Mode::Visual(_) => self.normal_key(k, ctx),
            Mode::Insert | Mode::Replace => self.insert_key(k, ctx),
            Mode::Cmdline(_) => self.cmdline_key(k, ctx),
        }
    }

    /// Общая работа после каждой клавиши.
    fn after_key(&mut self, _ctx: &mut Ctx) {
        self.cur = self.buf.clamp(self.cur);
        if matches!(self.mode, Mode::Normal | Mode::Visual(_)) {
            self.cur = self.clamp_normal(self.cur);
        }
        if !matches!(self.mode, Mode::Cmdline(_)) {
            self.track_cursor();
        }
        self.update_cursor_shape();
    }

    pub(crate) fn update_cursor_shape(&mut self) {
        match self.mode {
            Mode::Insert | Mode::Cmdline(_) => self.vs.normal_cursor(),
            _ => self.vs.block_cursor(),
        }
    }

    // ------------------------------------------------------------------
    // Режим вставки
    // ------------------------------------------------------------------

    /// Войти в режим вставки.
    pub(crate) fn start_insert(&mut self, replace: bool) {
        self.mode = if replace { Mode::Replace } else { Mode::Insert };
        self.insert_typed.clear();
        self.replaced.clear();
        self.completion = None;
        self.message = None;
        self.update_cursor_shape();
    }

    /// Вставить текст у курсора (заменяя в режиме замены).
    pub(crate) fn insert_text_at_cursor(&mut self, text: &str) {
        if self.mode == Mode::Replace {
            for c in text.chars() {
                if c == '\n' {
                    self.cur = self.buf.insert(self.cur, "\n");
                    continue;
                }
                let len = self.buf.line_len(self.cur.line);
                if self.cur.col < len {
                    let old = self.buf.line(self.cur.line)[self.cur.col];
                    self.buf.delete(self.cur, Pos::new(self.cur.line, self.cur.col + 1));
                    self.replaced.push(Some(old));
                } else {
                    self.replaced.push(None);
                }
                self.cur = self.buf.insert(self.cur, &c.to_string());
            }
        } else {
            self.cur = self.buf.insert(self.cur, text);
        }
        self.insert_typed.push_str(text);
    }

    /// Новая строка в режиме вставки (с автоотступом).
    pub(crate) fn insert_newline(&mut self) {
        let (ai, si) = {
            let o = &self.cfg.borrow().options;
            (o.autoindent, o.smartindent)
        };
        let line = self.cur.line;
        let before: String = self.buf.line(line)[..self.cur.col].iter().collect();
        let mut indent = if ai { self.indent_of(line) } else { String::new() };
        // Отступ не длиннее того, что стоит перед курсором.
        if indent.chars().count() > self.cur.col {
            indent = before.clone();
        }
        let t = before.trim_end();
        let python = self.lang.map(|l| l.id == "python" || l.id == "yaml").unwrap_or(false);
        let opens = t.ends_with('{') || t.ends_with('(') || t.ends_with('[') || (python && t.ends_with(':'));
        let mut extra = String::new();
        if si && opens {
            let sw = self.indent_params().1;
            let w = self.indent_width(line) + sw;
            extra = self.make_indent(w);
            indent.clear();
        }
        // Пробелы в конце строки перед переносом не оставляем, если строка — только отступ.
        let only_ws = before.chars().all(|c| c == ' ' || c == '\t') && !before.is_empty();
        if only_ws && ai {
            self.buf.delete(Pos::new(line, 0), self.cur);
            self.cur.col = 0;
        }
        let text = format!("\n{}{}", indent, extra);
        self.cur = self.buf.insert(self.cur, &text);
        // `{|}` → закрывающая скобка на своей строке.
        let rest = self.buf.line(self.cur.line);
        if si && opens && self.cur.col < rest.len() && matches!(rest[self.cur.col], '}' | ')' | ']') {
            let base = self.make_indent(self.indent_width(line));
            let save = self.cur;
            self.buf.insert(self.cur, &format!("\n{}", base));
            self.cur = save;
        }
        self.insert_typed.push('\n');
    }

    pub(crate) fn insert_key(&mut self, k: VKey, ctx: &mut Ctx) {
        if let Some(d) = &mut self.dot_pending {
            d.insert.push(k);
        }
        if self.await_reg {
            self.await_reg = false;
            if let VKey::Char(c) = k {
                if let Some(r) = self.get_reg(Some(c), ctx) {
                    let t = if r.linewise { format!("{}\n", r.text) } else { r.text };
                    self.insert_text_at_cursor(&t);
                }
            }
            return;
        }
        if !matches!(k, VKey::Ctrl('n') | VKey::Ctrl('p')) {
            self.completion = None;
        }
        let (ts, sw, et) = self.indent_params();
        match k {
            VKey::Esc => self.leave_insert(ctx),
            VKey::Char(c) => {
                self.insert_text_at_cursor(&c.to_string());
                // Закрывающая скобка в пустой строке — на уровень левее.
                let si = self.cfg.borrow().options.smartindent;
                if si && matches!(c, '}' | ']' | ')') && self.mode == Mode::Insert {
                    let l = self.cur.line;
                    let before = &self.buf.line(l)[..self.cur.col - 1];
                    if !before.is_empty() && before.iter().all(|c| *c == ' ' || *c == '\t') {
                        let w = self.indent_width(l).saturating_sub(sw);
                        let ind = self.make_indent(w);
                        let old = before.len();
                        self.buf.replace(Pos::new(l, 0), Pos::new(l, old), &ind);
                        self.cur.col = ind.chars().count() + 1;
                    }
                }
            }
            VKey::Enter => self.insert_newline(),
            VKey::Tab => {
                if et {
                    let x = self.col_to_x(self.cur.line, self.cur.col);
                    let n = sw - x % sw;
                    self.insert_text_at_cursor(&" ".repeat(n));
                } else {
                    self.insert_text_at_cursor("\t");
                }
            }
            VKey::Backspace | VKey::Ctrl('h') => {
                if self.mode == Mode::Replace {
                    if self.cur.col > 0 {
                        let p = Pos::new(self.cur.line, self.cur.col - 1);
                        match self.replaced.pop() {
                            Some(Some(old)) => {
                                self.buf.replace(p, self.cur, &old.to_string());
                            }
                            Some(None) => {
                                self.buf.delete(p, self.cur);
                            }
                            None => {}
                        }
                        self.cur = p;
                    }
                    return;
                }
                if self.cur.col > 0 {
                    // Мягкая табуляция: удалить пробелы до предыдущей позиции отступа.
                    let line = self.buf.line(self.cur.line);
                    let before = &line[..self.cur.col];
                    let mut n = 1;
                    if et && before.iter().all(|c| *c == ' ') {
                        let x = self.cur.col;
                        n = if x % sw == 0 { sw } else { x % sw };
                        n = n.min(x);
                    }
                    let a = Pos::new(self.cur.line, self.cur.col - n);
                    self.buf.delete(a, self.cur);
                    self.cur = a;
                } else if self.cur.line > 0 {
                    let pl = self.cur.line - 1;
                    let a = Pos::new(pl, self.buf.line_len(pl));
                    self.buf.delete(a, self.cur);
                    self.cur = a;
                }
                self.insert_typed.pop();
            }
            VKey::Del => {
                let len = self.buf.line_len(self.cur.line);
                if self.cur.col < len {
                    self.buf.delete(self.cur, Pos::new(self.cur.line, self.cur.col + 1));
                } else if self.cur.line + 1 < self.buf.line_count() {
                    self.buf.delete(self.cur, Pos::new(self.cur.line + 1, 0));
                }
            }
            VKey::Ctrl('w') => {
                let p = self.word_back_in_line(self.cur);
                if p < self.cur {
                    self.buf.delete(p, self.cur);
                    self.cur = p;
                } else if self.cur.line > 0 {
                    let pl = self.cur.line - 1;
                    let a = Pos::new(pl, self.buf.line_len(pl));
                    self.buf.delete(a, self.cur);
                    self.cur = a;
                }
            }
            VKey::Ctrl('u') => {
                let fnb = self.first_nonblank(self.cur.line);
                let start = if self.cur.col > fnb { fnb } else { 0 };
                let a = Pos::new(self.cur.line, start);
                self.buf.delete(a, self.cur);
                self.cur = a;
            }
            VKey::Ctrl('t') | VKey::Ctrl('d') => {
                let right = k == VKey::Ctrl('t');
                let l = self.cur.line;
                let old_w = self.indent_width(l);
                let new_w = if right { (old_w / sw + 1) * sw } else { old_w.saturating_sub(1) / sw * sw };
                let fnb = self.first_nonblank(l);
                let ind = self.make_indent(new_w);
                let new_len = ind.chars().count();
                self.buf.replace(Pos::new(l, 0), Pos::new(l, fnb), &ind);
                self.cur.col = if self.cur.col >= fnb { self.cur.col - fnb + new_len } else { new_len };
                self.cur = self.buf.clamp(self.cur);
            }
            VKey::Ctrl('r') => self.await_reg = true,
            VKey::Ctrl('o') => {
                self.one_shot = true;
                self.mode = Mode::Normal;
                self.update_cursor_shape();
            }
            VKey::Ctrl('n') | VKey::Ctrl('p') => self.complete_word(k == VKey::Ctrl('n')),
            VKey::Ctrl('e') | VKey::Ctrl('y') => {
                // Символ из строки ниже/выше.
                let l = if k == VKey::Ctrl('e') { self.cur.line + 1 } else { self.cur.line.wrapping_sub(1) };
                if l < self.buf.line_count() {
                    let x = self.col_to_x(self.cur.line, self.cur.col);
                    let col = self.x_to_col(l, x);
                    if let Some(&c) = self.buf.line(l).get(col) {
                        self.insert_text_at_cursor(&c.to_string());
                    }
                }
            }
            VKey::Ins => {
                self.mode = if self.mode == Mode::Insert { Mode::Replace } else { Mode::Insert };
                self.update_cursor_shape();
            }
            VKey::Left => self.cur.col = self.cur.col.saturating_sub(1),
            VKey::Right => self.cur.col = (self.cur.col + 1).min(self.buf.line_len(self.cur.line)),
            VKey::Up | VKey::Down => {
                let x = self.col_to_x(self.cur.line, self.cur.col);
                if k == VKey::Up && self.cur.line > 0 {
                    self.cur.line -= 1;
                } else if k == VKey::Down && self.cur.line + 1 < self.buf.line_count() {
                    self.cur.line += 1;
                }
                self.cur.col = self.x_to_col(self.cur.line, x);
            }
            VKey::Home => self.cur.col = 0,
            VKey::End => self.cur.col = self.buf.line_len(self.cur.line),
            VKey::PgUp | VKey::PgDn => {
                let h = self.text_height() as usize;
                self.cur.line = if k == VKey::PgUp { self.cur.line.saturating_sub(h) } else { (self.cur.line + h).min(self.buf.line_count() - 1) };
                self.cur = self.buf.clamp(self.cur);
            }
            _ => {}
        }
        let _ = ts;
    }

    /// Начало слова слева от `p` в той же строке (для Ctrl-W).
    fn word_back_in_line(&self, p: Pos) -> Pos {
        let l = self.buf.line(p.line);
        let mut i = p.col;
        while i > 0 && l[i - 1].is_whitespace() {
            i -= 1;
        }
        if i > 0 {
            let w = is_word_char(l[i - 1]);
            while i > 0 && !l[i - 1].is_whitespace() && is_word_char(l[i - 1]) == w {
                i -= 1;
            }
        }
        Pos::new(p.line, i)
    }

    /// Дополнение слова по словам из текста.
    fn complete_word(&mut self, forward: bool) {
        if self.completion.is_none() {
            let p = self.word_back_in_line(self.cur);
            let prefix: String = self.buf.line(self.cur.line)[p.col..self.cur.col].iter().collect();
            if prefix.is_empty() || !prefix.chars().all(is_word_char) {
                self.error("Нечего дополнять");
                return;
            }
            let mut items: Vec<String> = Vec::new();
            // Сначала слова ближе к курсору.
            let n = self.buf.line_count();
            let order = (0..n).map(|d| {
                let l = self.cur.line as isize + if d % 2 == 0 { (d / 2) as isize } else { -((d / 2 + 1) as isize) };
                l
            });
            for l in order.chain(0..n as isize) {
                if l < 0 || l as usize >= n {
                    continue;
                }
                let line = self.buf.line(l as usize);
                let mut i = 0;
                while i < line.len() {
                    if is_word_char(line[i]) && (i == 0 || !is_word_char(line[i - 1])) {
                        let mut j = i;
                        while j < line.len() && is_word_char(line[j]) {
                            j += 1;
                        }
                        let w: String = line[i..j].iter().collect();
                        let here = l as usize == self.cur.line && i == p.col;
                        if !here && w.len() > prefix.len() && w.starts_with(&prefix) && !items.contains(&w) {
                            items.push(w);
                        }
                        i = j;
                    } else {
                        i += 1;
                    }
                }
            }
            if items.is_empty() {
                self.error("Образец не найден");
                return;
            }
            self.completion = Some(Completion { start: p, prefix, items, index: None });
        }
        let c = self.completion.as_mut().unwrap();
        let n = c.items.len();
        c.index = match (c.index, forward) {
            (None, true) => Some(0),
            (None, false) => Some(n - 1),
            (Some(i), true) => {
                if i + 1 < n {
                    Some(i + 1)
                } else {
                    None
                }
            }
            (Some(i), false) => {
                if i > 0 {
                    Some(i - 1)
                } else {
                    None
                }
            }
        };
        let word = match c.index {
            Some(i) => c.items[i].clone(),
            None => c.prefix.clone(),
        };
        let start = c.start;
        let info = c.index.map(|i| format!("-- Дополнение: {} из {}", i + 1, n)).unwrap_or_else(|| "-- Исходный текст".into());
        self.buf.replace(start, self.cur, &word);
        self.cur = Pos::new(start.line, start.col + word.chars().count());
        self.info(&info);
    }

    /// Выход из режима вставки.
    pub(crate) fn leave_insert(&mut self, ctx: &mut Ctx) {
        // Повтор вставки (3ihi<Esc>, 3o).
        if self.insert_repeat > 1 && !self.insert_typed.is_empty() {
            let typed = self.insert_typed.clone();
            for _ in 1..self.insert_repeat {
                match self.insert_open_line {
                    Some(_) => {
                        let ind = self.indent_of(self.cur.line);
                        let len = self.buf.line_len(self.cur.line);
                        let end = self.buf.insert(Pos::new(self.cur.line, len), &format!("\n{}", ind));
                        self.cur = self.buf.insert(end, typed.trim_start_matches([' ', '\t']));
                    }
                    None => {
                        self.cur = self.buf.insert(self.cur, &typed);
                    }
                }
            }
        }
        // Вставка в блок: повторить набранное в остальных строках.
        if let Some(bi) = self.block_insert.take() {
            if self.cur.line == bi.start.line && self.cur.col > bi.start.col {
                let text: String = self.buf.line(bi.start.line)[bi.start.col..self.cur.col].iter().collect();
                for l in bi.first..=bi.last {
                    if l == bi.start.line {
                        continue;
                    }
                    if bi.x == usize::MAX {
                        let len = self.buf.line_len(l);
                        self.buf.insert(Pos::new(l, len), &text);
                        continue;
                    }
                    let lw = self.col_to_x(l, self.buf.line_len(l));
                    if lw < bi.x {
                        // Короткая строка: для A дополняем пробелами, для I пропускаем.
                        if !bi.append {
                            continue;
                        }
                        let pad = " ".repeat(bi.x - lw);
                        let len = self.buf.line_len(l);
                        self.buf.insert(Pos::new(l, len), &format!("{}{}", pad, text));
                        continue;
                    }
                    let col = if bi.x == usize::MAX { self.buf.line_len(l) } else { self.x_to_col(l, bi.x) };
                    self.buf.insert(Pos::new(l, col), &text);
                }
                self.cur = bi.start;
            }
        }
        if !self.insert_typed.is_empty() {
            let t = self.insert_typed.clone();
            self.registers.insert('.', Register { text: t, linewise: false });
        }
        self.insert_repeat = 1;
        self.insert_open_line = None;
        self.mode = Mode::Normal;
        self.last_insert = Some(self.cur);
        if self.cur.col > 0 {
            self.cur.col -= 1;
        }
        if let Some(mut d) = self.dot_pending.take() {
            if d.insert.last() != Some(&VKey::Esc) {
                d.insert.push(VKey::Esc);
            }
            if self.replay_depth == 0 || self.last_change.is_none() {
                self.last_change = Some(d);
            }
        }
        self.commit();
        self.completion = None;
        if self.message.as_ref().map(|m| m.0.starts_with("-- ")).unwrap_or(false) {
            self.message = None;
        }
        let _ = ctx;
        self.update_cursor_shape();
    }

    /// Закрыть группу отмены и запомнить место изменения.
    pub(crate) fn commit(&mut self) {
        if self.buf.has_open() {
            self.last_change_pos = Some(self.cur);
        }
        self.buf.commit();
    }

    // ------------------------------------------------------------------
    // Командная строка
    // ------------------------------------------------------------------

    /// Открыть командную строку (`:`, `/`, `?`) с начальным текстом.
    pub fn open_cmdline(&mut self, kind: CmdKind, initial: &str) {
        if !matches!(self.mode, Mode::Cmdline(_)) {
            self.prev_mode = self.mode;
        }
        self.mode = Mode::Cmdline(kind);
        self.cmdline = initial.chars().collect();
        self.cmd_pos = self.cmdline.len();
        self.hist_index = None;
        self.complete_state = None;
        self.message = None;
        self.search_origin = if kind == CmdKind::Ex { None } else { Some((self.cur, self.delta)) };
        self.update_cursor_shape();
    }

    fn close_cmdline(&mut self) {
        let search = matches!(self.mode, Mode::Cmdline(k) if k != CmdKind::Ex);
        self.mode = if matches!(self.prev_mode, Mode::Visual(_)) && !search { Mode::Normal } else { self.prev_mode };
        if matches!(self.mode, Mode::Cmdline(_)) {
            self.mode = Mode::Normal;
        }
        self.update_cursor_shape();
    }

    pub(crate) fn cmdline_key(&mut self, k: VKey, ctx: &mut Ctx) {
        let Mode::Cmdline(kind) = self.mode else { return };
        if self.await_reg {
            self.await_reg = false;
            if let VKey::Char(c) = k {
                let r = self.get_reg(Some(c), ctx);
                if let Some(r) = r {
                    for ch in r.text.chars().filter(|c| *c != '\n') {
                        self.cmdline.insert(self.cmd_pos, ch);
                        self.cmd_pos += 1;
                    }
                }
            } else if k == VKey::Ctrl('w') {
                // Ctrl-R Ctrl-W — слово под курсором.
                let w = self.word_under_cursor(false).map(|(w, _)| w).unwrap_or_default();
                for ch in w.chars() {
                    self.cmdline.insert(self.cmd_pos, ch);
                    self.cmd_pos += 1;
                }
            }
            self.incsearch_update();
            return;
        }
        if k != VKey::Tab {
            self.complete_state = None;
        }
        match k {
            VKey::Esc | VKey::Ctrl('c') => {
                self.op_after_search = None;
                if let Some((p, d)) = self.search_origin.take() {
                    self.cur = p;
                    self.delta = d;
                }
                self.close_cmdline();
            }
            VKey::Enter => {
                let text: String = self.cmdline.iter().collect();
                self.close_cmdline();
                match kind {
                    CmdKind::Ex => {
                        if !text.trim().is_empty() {
                            self.cmd_history.retain(|h| h != &text);
                            self.cmd_history.push(text.clone());
                        }
                        self.execute_ex(&text, ctx);
                    }
                    CmdKind::SearchFwd | CmdKind::SearchBack => {
                        let origin = self.search_origin.take();
                        if let Some((p, d)) = origin {
                            self.cur = p;
                            self.delta = d;
                        }
                        let fwd = kind == CmdKind::SearchFwd;
                        // Пустой образец — повтор последнего поиска.
                        let pat = if text.is_empty() { self.search.as_ref().map(|s| s.0.clone()).unwrap_or_default() } else { text.clone() };
                        if pat.is_empty() {
                            self.error("E35: Нет предыдущего образца поиска");
                        } else {
                            if !text.is_empty() {
                                self.search_history.retain(|h| h != &text);
                                self.search_history.push(text);
                            }
                            self.search = Some((pat, fwd));
                            self.noh = false;
                            let count = self.pending_search_count.take().unwrap_or(1);
                            if let Some((op, reg)) = self.op_after_search.take() {
                                // d/образец — от курсора до найденного (не включая).
                                let from = self.cur;
                                if let Some(p) = self.search_next(false, count, false) {
                                    let (a, b) = if p < from { (p, from) } else { (from, p) };
                                    let r = crate::normal::Range { a, b, kind: crate::normal::RKind::Char };
                                    self.apply(op, r, reg, 1, ctx);
                                    if !matches!(self.mode, Mode::Insert | Mode::Replace) {
                                        self.commit();
                                    }
                                }
                            } else {
                                self.search_next(false, count, true);
                            }
                        }
                    }
                }
            }
            VKey::Backspace | VKey::Ctrl('h') => {
                if self.cmdline.is_empty() {
                    if let Some((p, d)) = self.search_origin.take() {
                        self.cur = p;
                        self.delta = d;
                    }
                    self.close_cmdline();
                    return;
                }
                if self.cmd_pos > 0 {
                    self.cmd_pos -= 1;
                    self.cmdline.remove(self.cmd_pos);
                }
            }
            VKey::Del => {
                if self.cmd_pos < self.cmdline.len() {
                    self.cmdline.remove(self.cmd_pos);
                }
            }
            VKey::Left => self.cmd_pos = self.cmd_pos.saturating_sub(1),
            VKey::Right => self.cmd_pos = (self.cmd_pos + 1).min(self.cmdline.len()),
            VKey::Home | VKey::Ctrl('b') => self.cmd_pos = 0,
            VKey::End | VKey::Ctrl('e') => self.cmd_pos = self.cmdline.len(),
            VKey::Ctrl('u') => {
                self.cmdline.drain(..self.cmd_pos);
                self.cmd_pos = 0;
            }
            VKey::Ctrl('w') => {
                let mut i = self.cmd_pos;
                while i > 0 && self.cmdline[i - 1] == ' ' {
                    i -= 1;
                }
                while i > 0 && self.cmdline[i - 1] != ' ' {
                    i -= 1;
                }
                self.cmdline.drain(i..self.cmd_pos);
                self.cmd_pos = i;
            }
            VKey::Ctrl('r') => self.await_reg = true,
            VKey::Up | VKey::Down | VKey::Ctrl('p') | VKey::Ctrl('n') => {
                let up = matches!(k, VKey::Up | VKey::Ctrl('p'));
                let hist = if kind == CmdKind::Ex { self.cmd_history.clone() } else { self.search_history.clone() };
                if self.hist_index.is_none() {
                    self.hist_prefix = self.cmdline.iter().collect();
                }
                let prefix = self.hist_prefix.clone();
                let mut i = self.hist_index.unwrap_or(hist.len());
                loop {
                    if up {
                        if i == 0 {
                            break;
                        }
                        i -= 1;
                    } else {
                        i += 1;
                        if i >= hist.len() {
                            self.hist_index = None;
                            self.cmdline = prefix.chars().collect();
                            self.cmd_pos = self.cmdline.len();
                            break;
                        }
                    }
                    if hist[i].starts_with(&prefix) {
                        self.hist_index = Some(i);
                        self.cmdline = hist[i].chars().collect();
                        self.cmd_pos = self.cmdline.len();
                        break;
                    }
                }
            }
            VKey::Tab if kind == CmdKind::Ex => self.complete_cmdline(),
            VKey::Char(c) => {
                self.cmdline.insert(self.cmd_pos, c);
                self.cmd_pos += 1;
            }
            VKey::Tab => {
                self.cmdline.insert(self.cmd_pos, '\t');
                self.cmd_pos += 1;
            }
            _ => {}
        }
        self.incsearch_update();
    }

    /// Пошаговый поиск: курсор следует за набираемым образцом.
    fn incsearch_update(&mut self) {
        let Mode::Cmdline(kind) = self.mode else { return };
        if kind == CmdKind::Ex || !self.cfg.borrow().options.incsearch {
            return;
        }
        let Some((origin, d)) = self.search_origin else { return };
        self.cur = origin;
        self.delta = d;
        let pat: String = self.cmdline.iter().collect();
        if pat.is_empty() {
            return;
        }
        if let Ok(re) = self.make_regex(&pat) {
            if let Some((p, _)) = self.find_match(&re, origin, kind == CmdKind::SearchFwd, true) {
                self.cur = p;
                self.track_cursor();
            }
        }
    }

    /// Дополнение по Tab: имена параметров `:set`, команд, файлов.
    fn complete_cmdline(&mut self) {
        let text: String = self.cmdline[..self.cmd_pos].iter().collect();
        if let Some((prefix, items, idx)) = &mut self.complete_state {
            if items.is_empty() {
                return;
            }
            *idx = (*idx + 1) % items.len();
            let new = format!("{}{}", prefix, items[*idx]);
            self.cmdline = new.chars().collect();
            self.cmd_pos = self.cmdline.len();
            return;
        }
        let (head, word) = match text.rfind(' ') {
            Some(p) => (text[..=p].to_string(), text[p + 1..].to_string()),
            None => (String::new(), text.clone()),
        };
        let first = text.split_whitespace().next().unwrap_or("");
        let mut items: Vec<String> = Vec::new();
        if head.is_empty() {
            for c in crate::ex::COMMAND_NAMES {
                if c.starts_with(&word) {
                    items.push(c.to_string());
                }
            }
        } else if matches!(first, "set" | "se" | "setlocal" | "setl") {
            let (neg, w) = match word.strip_prefix("no") {
                Some(r) => ("no", r),
                None => ("", word.as_str()),
            };
            for o in crate::config::OPTIONS {
                if o.name.starts_with(w) {
                    items.push(format!("{}{}", neg, o.name));
                }
            }
            items.extend(["filetype=", "encoding=", "fileformat="].iter().filter(|s| s.starts_with(&word)).map(|s| s.to_string()));
        } else if matches!(first, "setf" | "setfiletype") || word.starts_with("ft=") || word.starts_with("filetype=") {
            let (pre, w) = match word.split_once('=') {
                Some((a, b)) => (format!("{}=", a), b.to_string()),
                None => (String::new(), word.clone()),
            };
            for l in crate::syntax::LANGS {
                if l.id.starts_with(&w) {
                    items.push(format!("{}{}", pre, l.id));
                }
            }
        } else if matches!(first, "colo" | "colorscheme") {
            for s in crate::config::SCHEMES {
                if s.name.starts_with(&word) {
                    items.push(s.name.to_string());
                }
            }
        } else {
            // Имена файлов.
            let (dir, name) = match word.rfind(['/', '\\']) {
                Some(p) => (word[..=p].to_string(), word[p + 1..].to_string()),
                None => (String::new(), word.clone()),
            };
            let base = if dir.is_empty() { PathBuf::from(".") } else { PathBuf::from(&dir) };
            if let Ok(rd) = std::fs::read_dir(&base) {
                let mut v: Vec<String> = rd
                    .flatten()
                    .filter_map(|e| {
                        let n = e.file_name().to_string_lossy().to_string();
                        if !n.starts_with(&name) {
                            return None;
                        }
                        let slash = if e.path().is_dir() { std::path::MAIN_SEPARATOR.to_string() } else { String::new() };
                        Some(format!("{}{}{}", dir, n, slash))
                    })
                    .collect();
                v.sort();
                items = v;
            }
        }
        if items.is_empty() {
            return;
        }
        let new = format!("{}{}", head, items[0]);
        self.cmdline = new.chars().collect();
        self.cmd_pos = self.cmdline.len();
        if items.len() > 1 {
            self.complete_state = Some((head, items, 0));
        }
    }

    // ------------------------------------------------------------------
    // Поиск
    // ------------------------------------------------------------------

    /// Регулярное выражение с учётом ignorecase/smartcase.
    pub fn make_regex(&self, pat: &str) -> Result<Regex, String> {
        let (ic, sc) = {
            let o = &self.cfg.borrow().options;
            (o.ignorecase, o.smartcase)
        };
        Regex::new(pat, ic, sc)
    }

    /// Найти совпадение от позиции `from` (не включая её); `wrap` — по кругу.
    /// Возвращает позицию и признак «перешли через край».
    pub fn find_match(&self, re: &Regex, from: Pos, forward: bool, wrap: bool) -> Option<(Pos, bool)> {
        let n = self.buf.line_count();
        let wrapscan = wrap && self.cfg.borrow().options.wrapscan;
        if forward {
            if let Some((a, _, _)) = re.find_at(self.buf.line(from.line), from.col + 1) {
                if a < self.buf.line_len(from.line) || self.buf.line_len(from.line) == 0 || a > from.col {
                    return Some((Pos::new(from.line, a), false));
                }
            }
            for d in 1..=n {
                let l = from.line + d;
                if l >= n && !wrapscan {
                    return None;
                }
                let l = l % n;
                if let Some((a, _, _)) = re.find_at(self.buf.line(l), 0) {
                    return Some((Pos::new(l, a), from.line + d >= n));
                }
            }
        } else {
            if let Some((a, _)) = re.find_before(self.buf.line(from.line), from.col) {
                return Some((Pos::new(from.line, a), false));
            }
            for d in 1..=n {
                if from.line < d && !wrapscan {
                    return None;
                }
                let l = (from.line + n - d) % n;
                let all = re.find_all(self.buf.line(l));
                if let Some(&(a, _)) = all.last() {
                    return Some((Pos::new(l, a), from.line < d));
                }
            }
        }
        None
    }

    /// `n` / `N`: следующее совпадение последнего поиска.
    pub fn search_next(&mut self, reverse: bool, count: usize, show: bool) -> Option<Pos> {
        let Some((pat, fwd)) = self.search.clone() else {
            self.error("E35: Нет предыдущего образца поиска");
            return None;
        };
        let re = match self.make_regex(&pat) {
            Ok(r) => r,
            Err(e) => {
                self.error(&e);
                return None;
            }
        };
        let forward = fwd != reverse;
        let mut p = self.cur;
        let mut wrapped = false;
        for _ in 0..count.max(1) {
            match self.find_match(&re, p, forward, true) {
                Some((np, w)) => {
                    p = np;
                    wrapped |= w;
                }
                None => {
                    self.error(&format!("E486: Образец не найден: {}", pat));
                    return None;
                }
            }
        }
        self.noh = false;
        if show {
            self.last_jump = Some(self.cur);
            self.cur = p;
            if wrapped {
                self.error(if forward {
                    "Поиск дошёл до КОНЦА и продолжен с НАЧАЛА"
                } else {
                    "Поиск дошёл до НАЧАЛА и продолжен с КОНЦА"
                });
            } else {
                self.info(&format!("{}{}", if forward { '/' } else { '?' }, pat));
            }
        }
        Some(p)
    }

    /// Слово под курсором (или следующее в строке) и его начало.
    pub fn word_under_cursor(&self, big: bool) -> Option<(String, usize)> {
        let l = self.buf.line(self.cur.line);
        let mut i = self.cur.col;
        let isw = |c: char| if big { !c.is_whitespace() } else { is_word_char(c) };
        while i < l.len() && !isw(l[i]) {
            i += 1;
        }
        if i >= l.len() {
            return None;
        }
        let mut a = i;
        while a > 0 && isw(l[a - 1]) {
            a -= 1;
        }
        let mut b = i;
        while b < l.len() && isw(l[b]) {
            b += 1;
        }
        Some((l[a..b].iter().collect(), a))
    }

    // ------------------------------------------------------------------
    // Отрисовка
    // ------------------------------------------------------------------

    /// Выделение визуального режима: начало, конец (включительно), вид.
    pub fn visual_range(&self) -> Option<(Pos, Pos, VKind)> {
        match self.mode {
            Mode::Visual(k) => {
                let (a, b) = if self.vstart <= self.cur { (self.vstart, self.cur) } else { (self.cur, self.vstart) };
                Some((a, b, k))
            }
            Mode::Cmdline(_) => match self.prev_mode {
                Mode::Visual(k) => {
                    let (a, b) = if self.vstart <= self.cur { (self.vstart, self.cur) } else { (self.cur, self.vstart) };
                    Some((a, b, k))
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Активный образец для подсветки найденного.
    fn highlight_regex(&self) -> Option<Regex> {
        if !self.cfg.borrow().options.hlsearch {
            if let Mode::Cmdline(k) = self.mode {
                if k != CmdKind::Ex && self.cfg.borrow().options.incsearch {
                    let p: String = self.cmdline.iter().collect();
                    return (!p.is_empty()).then(|| self.make_regex(&p).ok()).flatten();
                }
            }
            return None;
        }
        if let Mode::Cmdline(k) = self.mode {
            if k != CmdKind::Ex && self.cfg.borrow().options.incsearch {
                let p: String = self.cmdline.iter().collect();
                if !p.is_empty() {
                    return self.make_regex(&p).ok();
                }
            }
        }
        if self.noh {
            return None;
        }
        self.search.as_ref().and_then(|(p, _)| self.make_regex(p).ok())
    }

    /// Парная скобка к скобке под курсором (для подсветки).
    fn match_paren(&self) -> Option<(Pos, Pos)> {
        if !self.cfg.borrow().options.showmatch {
            return None;
        }
        let l = self.buf.line(self.cur.line);
        let mut col = self.cur.col;
        if matches!(self.mode, Mode::Insert) && (col >= l.len() || !"()[]{}".contains(l[col])) && col > 0 {
            col -= 1;
        }
        let c = *l.get(col)?;
        if !"()[]{}".contains(c) {
            return None;
        }
        let p = Pos::new(self.cur.line, col);
        self.find_pair(p, 3000).map(|q| (p, q))
    }

    fn draw_editor(&self, dc: &mut DrawCtx) {
        let cfg = self.cfg.borrow();
        let o = cfg.options.clone();
        let w = self.vs.size.x.max(1) as usize;
        let h = self.text_height();
        let gw = self.gutter_width();
        let tw = w.saturating_sub(gw);
        let ts = self.tabstop();
        let normal = cfg.color(Elem::Normal);
        let cl_bg = cfg.color(Elem::CursorLine).bg;
        let visual = self.visual_range();
        let show_cl = o.cursorline && visual.is_none();
        let re = self.highlight_regex();
        let paren = self.match_paren();
        let n = self.buf.line_count();
        let dx = self.delta.x.max(0) as usize;
        let list = o.list;
        let cc = o.colorcolumn;
        let in_visual = |li: usize, col: usize, x: usize| -> bool {
            match visual {
                Some((a, b, VKind::Char)) => Pos::new(li, col) >= a && Pos::new(li, col) <= b,
                Some((a, b, VKind::Line)) => li >= a.line && li <= b.line,
                Some((a, b, VKind::Block)) => {
                    if li < a.line || li > b.line {
                        return false;
                    }
                    let xa = self.col_to_x(self.vstart.line, self.vstart.col);
                    let xb = self.col_to_x(self.cur.line, self.cur.col);
                    let (l, r) = if xa <= xb { (xa, xb) } else { (xb, xa) };
                    if self.want_x == Some(usize::MAX) {
                        x >= l
                    } else {
                        x >= l && x <= r
                    }
                }
                None => false,
            }
        };
        drop(cfg);
        for row in 0..h {
            let cfg = self.cfg.borrow();
            let li = self.delta.y as usize + row as usize;
            let mut b = DrawBuffer::new(w);
            b.move_char(0, ' ', normal.attr(), w);
            if li >= n {
                b.move_char(0, '~', cfg.attr(Elem::NonText), 1);
                dc.write_line(0, row, w as i32, 1, &b);
                continue;
            }
            let is_cur = li == self.cur.line;
            let line_bg = if show_cl && is_cur { cl_bg } else { normal.bg };
            // Номера строк.
            if gw > 0 {
                let num = if o.relativenumber && !is_cur { (li as isize - self.cur.line as isize).unsigned_abs() } else { li + 1 };
                let s = if o.relativenumber && is_cur && !o.number {
                    format!("{:<width$}", 0, width = gw - 1)
                } else if o.relativenumber && is_cur {
                    format!("{:<width$}", num, width = gw - 1)
                } else {
                    format!("{:>width$}", num, width = gw - 1)
                };
                let attr = if is_cur { cfg.attr(Elem::CursorLineNr) } else { cfg.attr(Elem::LineNr) };
                b.move_str(0, &s, attr);
                b.move_char(gw - 1, ' ', attr, 1);
            }
            let base_attr = crate::config::Color::new(normal.fg, line_bg).attr();
            b.move_char(gw, ' ', base_attr, tw);
            if cc > dx && cc - 1 < dx + tw {
                let c = cfg.color(Elem::ColorColumn);
                b.move_char(gw + cc - 1 - dx, ' ', crate::config::Color::new(normal.fg, c.bg).attr(), 1);
            }
            let line = self.buf.line(li);
            let toks = self.syntax_line(li);
            let matches = re.as_ref().map(|r| r.find_all(line)).unwrap_or_default();
            let trail_from = line.iter().rposition(|c| !c.is_whitespace()).map(|p| p + 1).unwrap_or(0);
            let mut x = 0usize;
            for (ci, &c) in line.iter().enumerate() {
                let cw = char_cells(c, x, ts);
                if x >= dx + tw {
                    break;
                }
                let e = toks.get(ci).copied().unwrap_or(Elem::Normal);
                let tc = cfg.color(e);
                let bg = if tc.bg != normal.bg { tc.bg } else { line_bg };
                let mut bg = if cc > 0 && x == cc - 1 && bg == line_bg { cfg.color(Elem::ColorColumn).bg } else { bg };
                let mut fg = tc.fg;
                let mut attr = crate::config::Color::new(fg, bg).attr();
                if matches.iter().any(|&(a, bb)| ci >= a && ci < bb) {
                    attr = cfg.attr(Elem::Search);
                }
                if paren.map(|(p, q)| (p.line == li && p.col == ci) || (q.line == li && q.col == ci)).unwrap_or(false) {
                    attr = cfg.attr(Elem::MatchParen);
                }
                if in_visual(li, ci, x) {
                    attr = cfg.attr(Elem::Visual);
                }
                let vis_a = x.max(dx);
                let vis_b = (x + cw).min(dx + tw);
                if vis_a < vis_b {
                    let sx = gw + vis_a - dx;
                    if c == '\t' {
                        if list {
                            let nt = cfg.color(Elem::NonText);
                            fg = nt.fg;
                            let a2 = if in_visual(li, ci, x) { attr } else { crate::config::Color::new(fg, bg).attr() };
                            b.move_char(sx, ' ', a2, vis_b - vis_a);
                            if vis_a == x {
                                b.move_char(sx, '»', a2, 1);
                            }
                        } else {
                            b.move_char(sx, ' ', attr, vis_b - vis_a);
                        }
                    } else if vis_b - vis_a < cw {
                        b.move_char(sx, ' ', attr, vis_b - vis_a);
                    } else if list && c == ' ' && ci >= trail_from {
                        let nt = cfg.color(Elem::NonText);
                        if !in_visual(li, ci, x) {
                            bg = line_bg;
                            attr = crate::config::Color::new(nt.fg, bg).attr();
                        }
                        b.move_char(sx, '·', attr, 1);
                    } else if (c as u32) < 32 {
                        // Управляющий символ — как ^X.
                        b.move_char(sx, '^', cfg.attr(Elem::Special), 1);
                    } else {
                        let mut tmp = [0u8; 4];
                        b.move_str(sx, c.encode_utf8(&mut tmp), attr);
                    }
                }
                x += cw;
            }
            // Выделение переводов строк.
            if x >= dx && x < dx + tw {
                let eol_sel = match visual {
                    Some((a, bb, VKind::Char)) => li >= a.line && li < bb.line,
                    Some((a, bb, VKind::Line)) => li >= a.line && li <= bb.line,
                    _ => false,
                };
                if eol_sel {
                    b.move_char(gw + x - dx, ' ', cfg.attr(Elem::Visual), 1);
                }
            }
            dc.write_line(0, row, w as i32, 1, &b);
        }
        self.draw_status(dc, h, w);
    }

    /// Строка состояния и командная строка.
    fn draw_status(&self, dc: &mut DrawCtx, y: i32, w: usize) {
        let cfg = self.cfg.borrow();
        if self.vs.size.y < BOTTOM_ROWS + 1 {
            return;
        }
        let st = cfg.attr(Elem::StatusLine);
        let mut b = DrawBuffer::new(w);
        b.move_char(0, ' ', st, w);
        let (mode_name, me) = match self.mode {
            Mode::Normal if self.one_shot => ("(ВСТАВКА)", Elem::ModeInsert),
            Mode::Normal => ("NORMAL", Elem::ModeNormal),
            Mode::Insert => ("INSERT", Elem::ModeInsert),
            Mode::Replace => ("REPLACE", Elem::ModeReplace),
            Mode::Visual(VKind::Char) => ("VISUAL", Elem::ModeVisual),
            Mode::Visual(VKind::Line) => ("V-LINE", Elem::ModeVisual),
            Mode::Visual(VKind::Block) => ("V-BLOCK", Elem::ModeVisual),
            Mode::Cmdline(_) => ("COMMAND", Elem::ModeNormal),
        };
        let badge = format!(" {} ", mode_name);
        let mut x = b.move_str(0, &badge, cfg.attr(me));
        let mut left = format!(" {}", self.short_name());
        if self.modified() {
            left.push_str(" [+]");
        }
        if self.readonly {
            left.push_str(" [RO]");
        }
        if let Some((r, _)) = &self.recording {
            left.push_str(&format!("  запись @{}", r));
        }
        x += b.move_str_limited(x, &left, st, w.saturating_sub(x));
        let total = self.buf.line_count();
        let pct = if total <= 1 { 100 } else { self.cur.line * 100 / (total - 1) };
        let col = self.cur.col + 1;
        let vcol = self.col_to_x(self.cur.line, self.cur.col) + 1;
        let colstr = if vcol != col { format!("{}-{}", col, vcol) } else { col.to_string() };
        let right = format!(
            " {} │ {} │ {} │ {}:{} │ {:>3}% ",
            self.lang.map(|l| l.id).unwrap_or("text"),
            self.encoding.name(),
            if self.crlf { "dos" } else { "unix" },
            self.cur.line + 1,
            colstr,
            pct
        );
        let rw = tvision::text::str_width(&right);
        if x + rw <= w {
            b.move_str(w - rw, &right, st);
        }
        dc.write_line(0, y, w as i32, 1, &b);

        // Командная строка.
        let ca = cfg.attr(Elem::CmdLine);
        let mut b = DrawBuffer::new(w);
        b.move_char(0, ' ', ca, w);
        match self.mode {
            Mode::Cmdline(k) => {
                let prefix = match k {
                    CmdKind::Ex => ':',
                    CmdKind::SearchFwd => '/',
                    CmdKind::SearchBack => '?',
                };
                let text: String = self.cmdline.iter().collect();
                let s = format!("{}{}", prefix, text);
                // Длинная строка прокручивается так, чтобы курсор был виден.
                let cx = 1 + tvision::text::str_width(&self.cmdline[..self.cmd_pos].iter().collect::<String>());
                let skip = cx.saturating_sub(w.saturating_sub(1));
                let visible: String = s.chars().skip(skip).collect();
                b.move_str(0, &visible, ca);
            }
            _ => {
                if let Some((m, err)) = &self.message {
                    let a = if *err { cfg.attr(Elem::ErrorMsg) } else { ca };
                    let first = m.lines().next().unwrap_or("");
                    b.move_str_limited(0, first, a, w.saturating_sub(12));
                }
                if cfg.options.showcmd {
                    let mut pend = crate::keys::format_keys(&self.map_pending);
                    pend.push_str(&crate::keys::format_keys(&self.pending));
                    if let Some((a, bb, k)) = self.visual_range() {
                        if pend.is_empty() {
                            pend = match k {
                                VKind::Line => format!("{}", bb.line - a.line + 1),
                                VKind::Char if a.line == bb.line => format!("{}", bb.col - a.col + 1),
                                VKind::Char => format!("{}", bb.line - a.line + 1),
                                VKind::Block => {
                                    let xa = self.col_to_x(self.vstart.line, self.vstart.col);
                                    let xb = self.col_to_x(self.cur.line, self.cur.col);
                                    format!("{}x{}", bb.line - a.line + 1, xa.abs_diff(xb) + 1)
                                }
                            };
                        }
                    }
                    let pw = tvision::text::str_width(&pend).min(10);
                    if pw > 0 && w > 12 {
                        b.move_str_limited(w - 11, &pend, ca, 10);
                    }
                }
            }
        }
        dc.write_line(0, y + 1, w as i32, 1, &b);
    }

    /// Положение курсора на экране.
    fn update_cursor(&mut self) {
        if let Mode::Cmdline(_) = self.mode {
            let w = self.vs.size.x.max(1) as usize;
            let cx = 1 + tvision::text::str_width(&self.cmdline[..self.cmd_pos].iter().collect::<String>());
            let cx = cx.min(w.saturating_sub(1));
            self.vs.set_cursor(cx as i32, self.vs.size.y - 1);
            return;
        }
        let gw = self.gutter_width() as i32;
        let x = self.col_to_x(self.cur.line, self.cur.col) as i32 - self.delta.x + gw;
        let y = self.cur.line as i32 - self.delta.y;
        self.vs.set_cursor(x, y);
    }

    /// Сообщить полосе прокрутки положение.
    pub(crate) fn sync(&mut self, ctx: &mut Ctx) {
        let max_y = (self.buf.line_count() as i32 - 1).max(0);
        self.delta.y = self.delta.y.clamp(0, max_y);
        self.update_cursor();
        let key = (self.delta.y, self.buf.line_count() as i32, self.text_height());
        if self.last_sync == Some(key) {
            return;
        }
        self.last_sync = Some(key);
        if let Some(id) = self.v_scroll {
            let h = self.text_height();
            ctx.message(cm::SCROLL_BAR_SET, Info::Scroll { id, value: self.delta.y, min: 0, max: max_y, page: (h - 1).max(1), arrow: 1 });
        }
    }

    /// Позиция текста под мышью (`None` — вне текста).
    fn pos_at(&self, mouse: Point) -> Option<Pos> {
        let p = self.vs.make_local(mouse);
        if p.y < 0 || p.y >= self.text_height() {
            return None;
        }
        let line = (self.delta.y + p.y).clamp(0, self.buf.line_count() as i32 - 1) as usize;
        let x = (p.x - self.gutter_width() as i32 + self.delta.x).max(0) as usize;
        Some(Pos::new(line, self.x_to_col(line, x)))
    }

    // ------------------------------------------------------------------
    // Команды меню
    // ------------------------------------------------------------------

    /// Команда tvision (из меню) — в терминах vim.
    fn menu_command(&mut self, c: u16, ctx: &mut Ctx) -> bool {
        match c {
            cm::SAVE => {
                if self.file.as_os_str().is_empty() {
                    self.save_as_dialog(ctx);
                } else {
                    self.execute_ex("w", ctx);
                }
            }
            cm::SAVE_AS => self.save_as_dialog(ctx),
            cm::UNDO => self.run_keys(&[VKey::Esc, VKey::Char('u')], ctx),
            cm::REDO => self.run_keys(&[VKey::Esc, VKey::Ctrl('r')], ctx),
            cm::COPY => {
                if matches!(self.mode, Mode::Visual(_)) {
                    self.run_keys(&[VKey::Char('"'), VKey::Char('+'), VKey::Char('y')], ctx);
                } else {
                    self.run_keys(&[VKey::Char('"'), VKey::Char('+'), VKey::Char('y'), VKey::Char('y')], ctx);
                }
            }
            cm::CUT => {
                if matches!(self.mode, Mode::Visual(_)) {
                    self.run_keys(&[VKey::Char('"'), VKey::Char('+'), VKey::Char('d')], ctx);
                } else {
                    self.run_keys(&[VKey::Char('"'), VKey::Char('+'), VKey::Char('d'), VKey::Char('d')], ctx);
                }
            }
            cm::PASTE => {
                if matches!(self.mode, Mode::Insert | Mode::Replace) {
                    if let Some(r) = self.get_reg(Some('+'), ctx) {
                        self.insert_text_at_cursor(&r.text);
                    }
                } else {
                    self.run_keys(&[VKey::Char('"'), VKey::Char('+'), VKey::Char('P')], ctx);
                }
            }
            cm::CLEAR => {
                if matches!(self.mode, Mode::Visual(_)) {
                    self.run_keys(&[VKey::Char('"'), VKey::Char('_'), VKey::Char('d')], ctx);
                }
            }
            cm::SELECT_ALL => self.run_keys(&[VKey::Esc, VKey::Char('g'), VKey::Char('g'), VKey::Char('V'), VKey::Char('G')], ctx),
            cm::FIND => {
                self.run_keys(&[VKey::Esc], ctx);
                self.open_cmdline(CmdKind::SearchFwd, "");
            }
            cm::REPLACE => {
                self.run_keys(&[VKey::Esc], ctx);
                self.open_cmdline(CmdKind::Ex, "%s//g");
                self.cmd_pos = 3;
            }
            cm::SEARCH_AGAIN => self.run_keys(&[VKey::Char('n')], ctx),
            cmd::GOTO_LINE => {
                self.run_keys(&[VKey::Esc], ctx);
                self.open_cmdline(CmdKind::Ex, "");
            }
            _ => return false,
        }
        self.after_key(ctx);
        true
    }

    /// Выполнить клавиши так, будто их нажали (без записи в макрос).
    pub fn run_keys(&mut self, keys: &[VKey], ctx: &mut Ctx) {
        for &k in keys {
            self.process(k, ctx);
        }
    }

    /// Диалог «Сохранить как».
    pub fn save_as_dialog(&mut self, ctx: &mut Ctx) {
        let d = FileDialog::new("*", "Сохранить файл как", "~И~мя", fd::OK_BUTTON, 101);
        ctx.exec(Box::new(d), Some(self.vs.id), tag::SAVE_AS);
    }

    /// Показать окно с сообщением.
    pub fn show_box(&mut self, text: &str, ctx: &mut Ctx) {
        let lines = text.lines().count() as i32;
        let width = text.lines().map(tvision::text::str_width).max().unwrap_or(10).clamp(30, 76) as i32 + 4;
        let h = (lines + 6).min(ctx.screen_size.y - 2).max(8);
        let d = build_message_box(Rect::new(0, 0, width, h), text, mf::INFORMATION | mf::OK_BUTTON);
        ctx.exec(Box::new(d), Some(self.vs.id), tag::INFO);
    }
}

/// Ширина символа на экране с учётом табуляции.
pub fn char_cells(c: char, x: usize, ts: usize) -> usize {
    if c == '\t' {
        ts - x % ts
    } else {
        tvision::text::char_width(c).max(1)
    }
}

/// Символ слова (буква, цифра, `_`).
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Абсолютный путь без обращения к ФС.
pub fn absolute(p: &Path) -> PathBuf {
    let p = if let Some(rest) = p.to_str().and_then(|s| s.strip_prefix("~/")) {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| PathBuf::from(h).join(rest)).unwrap_or_else(|| p.to_path_buf())
    } else {
        p.to_path_buf()
    };
    let p = if p.is_absolute() { p } else { std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p) };
    // Убрать «.» и «..» (без обращения к файловой системе).
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

fn clip_text(r: &Register) -> String {
    if r.linewise {
        format!("{}\n", r.text)
    } else {
        r.text.clone()
    }
}

fn from_clip(s: &str) -> Register {
    let s = s.replace("\r\n", "\n");
    match s.strip_suffix('\n') {
        Some(t) => Register { text: t.to_string(), linewise: true },
        None => Register { text: s, linewise: false },
    }
}

impl View for VimEditor {
    tvision::impl_view_basics!();

    fn draw(&self, dc: &mut DrawCtx) {
        self.draw_editor(dc);
    }

    fn handle_event(&mut self, ev: &mut Event, ctx: &mut Ctx) {
        default_handle_event(&self.vs, ev, ctx);
        let id = self.vs.id;
        // Настройки поменялись — пересчитать прокрутку.
        let gen = self.cfg.borrow().generation;
        if gen != self.last_cfg_gen {
            self.last_cfg_gen = gen;
            self.last_sync = None;
        }
        match ev {
            Event::KeyDown(k) if self.vs.is_focused() => {
                if let Some(vk) = VKey::from_event(*k) {
                    // Esc в обычном режиме без набранной команды — сбросить сообщение.
                    self.key(vk, ctx);
                    ev.clear();
                }
            }
            Event::Command(m) if self.vs.is_focused() => {
                if self.menu_command(m.command, ctx) {
                    ev.clear();
                }
            }
            Event::MouseDown(m) => {
                let m = *m;
                if let Some(p) = self.pos_at(m.pos) {
                    if matches!(self.mode, Mode::Cmdline(_)) {
                        self.close_cmdline();
                    }
                    if m.double {
                        // Двойной щелчок — выделить слово.
                        self.cur = p;
                        if let Some((w, a)) = self.word_under_cursor(false) {
                            self.vstart = Pos::new(p.line, a);
                            self.cur = Pos::new(p.line, a + w.chars().count() - 1);
                            self.mode = Mode::Visual(VKind::Char);
                        }
                    } else {
                        if matches!(self.mode, Mode::Visual(_)) && !m.mods.shift {
                            self.exit_visual();
                        }
                        if m.mods.shift && !matches!(self.mode, Mode::Visual(_)) {
                            self.vstart = self.cur;
                            self.mode = Mode::Visual(VKind::Char);
                        }
                        self.cur = p;
                        self.drag_from = Some(p);
                        ctx.grab_mouse(id);
                    }
                    self.pending.clear();
                    self.want_x = None;
                    self.after_key(ctx);
                }
                ev.clear();
            }
            Event::MouseMove(m) | Event::MouseAuto(m) if ctx.mouse_grabbed_by(id) => {
                let local = self.vs.make_local(m.pos);
                if local.y < 0 {
                    self.delta.y = (self.delta.y - 1).max(0);
                } else if local.y >= self.text_height() {
                    self.delta.y += 1;
                }
                let mut mp = m.pos;
                let top = self.vs.origin.y;
                mp.y = mp.y.clamp(top, top + self.text_height() - 1);
                if let (Some(p), Some(from)) = (self.pos_at(mp), self.drag_from) {
                    if p != from && !matches!(self.mode, Mode::Visual(_)) {
                        if matches!(self.mode, Mode::Insert | Mode::Replace) {
                            self.leave_insert(ctx);
                        }
                        self.vstart = from;
                        self.mode = Mode::Visual(VKind::Char);
                    }
                    self.cur = p;
                    self.after_key(ctx);
                }
                ev.clear();
            }
            Event::MouseUp(_) if ctx.mouse_grabbed_by(id) => {
                ctx.release_mouse();
                self.drag_from = None;
                ev.clear();
            }
            Event::MouseWheel(m) => {
                let dy = m.wheel as i32 * 3;
                let max_y = (self.buf.line_count() as i32 - 1).max(0);
                self.delta.y = (self.delta.y + dy).clamp(0, max_y);
                self.keep_cursor_in_view();
                ev.clear();
            }
            Event::Broadcast(m) => match (&m.command, &m.info) {
                (&cm::SCROLL_BAR_CHANGED, Info::Scroll { id: sb, value, .. }) if Some(*sb) == self.v_scroll => {
                    self.delta.y = *value;
                    self.keep_cursor_in_view();
                    self.last_sync = None;
                }
                (&cm::SCROLL_BAR_CLICKED, Info::Id(sb)) if Some(*sb) == self.v_scroll => {
                    if !self.vs.is_selected() {
                        ctx.select(id);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        self.sync(ctx);
    }

    fn exec_result(&mut self, tag_: u16, command: u16, view: Box<dyn View>, ctx: &mut Ctx) {
        match tag_ {
            tag::SAVE_AS => {
                if command != cm::CANCEL {
                    if let Ok(d) = view.into_any().downcast::<FileDialog>() {
                        let name = d.file_name();
                        if !name.as_os_str().is_empty() {
                            let was_unnamed = self.file.as_os_str().is_empty();
                            self.file = absolute(&name);
                            if was_unnamed {
                                self.detect_lang();
                            }
                            match self.write_file(None) {
                                Ok(m) => {
                                    self.info(&m);
                                    if let Some(c) = self.pending_close.take() {
                                        self.force_close = true;
                                        ctx.post_command(c, Info::None);
                                    } else if self.close_after_save {
                                        self.force_close = true;
                                        ctx.post_command(cm::CLOSE, Info::None);
                                        ctx.post_command(cmd::QUIT_IF_EMPTY, Info::None);
                                    }
                                }
                                Err(e) => self.error(&e),
                            }
                        }
                    }
                } else {
                    self.pending_close = None;
                }
                self.close_after_save = false;
            }
            tag::SAVE_MODIFY => {
                let pending = self.pending_close.take();
                match command {
                    cm::YES => {
                        if self.file.as_os_str().is_empty() {
                            self.pending_close = pending;
                            self.save_as_dialog(ctx);
                        } else {
                            match self.write_file(None) {
                                Ok(_) => {
                                    if let Some(c) = pending {
                                        ctx.post_command(c, Info::None);
                                    }
                                }
                                Err(e) => self.show_box(&e, ctx),
                            }
                        }
                    }
                    cm::NO => {
                        self.force_close = true;
                        if let Some(c) = pending {
                            ctx.post_command(c, Info::None);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.sync(ctx);
    }

    fn update_commands(&self, ctx: &mut Ctx) {
        let f = self.vs.is_focused();
        let vis = matches!(self.mode, Mode::Visual(_));
        ctx.set_command(cm::SAVE, f);
        ctx.set_command(cm::SAVE_AS, f);
        ctx.set_command(cm::UNDO, f && self.buf.can_undo());
        ctx.set_command(cm::REDO, f && self.buf.can_redo());
        ctx.set_command(cm::CUT, f);
        ctx.set_command(cm::COPY, f);
        ctx.set_command(cm::CLEAR, f && vis);
        ctx.set_command(cm::PASTE, f);
        ctx.set_command(cm::FIND, f);
        ctx.set_command(cm::REPLACE, f);
        ctx.set_command(cm::SEARCH_AGAIN, f && self.search.is_some());
        ctx.set_command(cm::SELECT_ALL, f);
    }

    /// При закрытии изменённого файла — вопрос «Сохранить?».
    fn valid(&mut self, command: u16, ctx: &mut Ctx) -> bool {
        if command == cm::VALID || self.force_close || self.readonly {
            return true;
        }
        if !self.modified() || !matches!(command, cm::CLOSE | cm::QUIT) {
            return true;
        }
        let msg = format!("\x03Файл {} изменён.\n\x03Сохранить изменения?", self.short_name());
        let d = build_message_box(Rect::new(0, 0, 50, 9), &msg, mf::CONFIRMATION | mf::YES_NO_CANCEL);
        self.pending_close = Some(command);
        ctx.exec(Box::new(d), Some(self.vs.id), tag::SAVE_MODIFY);
        false
    }

    fn change_bounds(&mut self, bounds: Rect) {
        self.vs.set_bounds(bounds);
        self.last_sync = None;
        self.track_cursor();
        self.update_cursor();
    }
}
