//! Настройки редактора: параметры (`:set`), цветовая схема, типы файлов,
//! сопоставления клавиш (`:map`).
//!
//! Всё хранится в общем объекте [`Config`] (через `Rc<RefCell<…>>`): окна
//! редактора читают его при каждой отрисовке, поэтому изменения из
//! диалогов видны сразу во всех окнах. На диск настройки пишутся в файл
//! настроек tvision-rs ([`tvision::settings::Settings`]) в поле `extra`
//! под ключом `"tvim"`.

use std::cell::RefCell;
use std::rc::Rc;

use tvision::color::{attr, Attr};
use tvision::json::Json;

use crate::keys::{parse_keys, VKey};
use crate::syntax::LANGS;

/// Общие настройки.
pub type Shared = Rc<RefCell<Config>>;

// ----------------------------------------------------------------------
// Элементы цветовой схемы
// ----------------------------------------------------------------------

/// Элемент цветовой схемы: и лексемы подсветки, и части интерфейса.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum Elem {
    Normal = 0,
    Comment,
    Keyword,
    Type,
    Constant,
    String,
    Number,
    Preproc,
    Function,
    Operator,
    Special,
    Todo,
    LineNr,
    CursorLineNr,
    CursorLine,
    Visual,
    Search,
    MatchParen,
    NonText,
    ColorColumn,
    ModeNormal,
    ModeInsert,
    ModeVisual,
    ModeReplace,
    StatusLine,
    CmdLine,
    ErrorMsg,
}

/// Число элементов схемы.
pub const ELEM_COUNT: usize = 27;

impl Elem {
    pub const ALL: [Elem; ELEM_COUNT] = [
        Elem::Normal,
        Elem::Comment,
        Elem::Keyword,
        Elem::Type,
        Elem::Constant,
        Elem::String,
        Elem::Number,
        Elem::Preproc,
        Elem::Function,
        Elem::Operator,
        Elem::Special,
        Elem::Todo,
        Elem::LineNr,
        Elem::CursorLineNr,
        Elem::CursorLine,
        Elem::Visual,
        Elem::Search,
        Elem::MatchParen,
        Elem::NonText,
        Elem::ColorColumn,
        Elem::ModeNormal,
        Elem::ModeInsert,
        Elem::ModeVisual,
        Elem::ModeReplace,
        Elem::StatusLine,
        Elem::CmdLine,
        Elem::ErrorMsg,
    ];

    /// Название для диалога цветов.
    pub fn title(self) -> &'static str {
        match self {
            Elem::Normal => "Обычный текст",
            Elem::Comment => "Комментарий",
            Elem::Keyword => "Ключевое слово",
            Elem::Type => "Тип",
            Elem::Constant => "Константа",
            Elem::String => "Строка",
            Elem::Number => "Число",
            Elem::Preproc => "Препроцессор",
            Elem::Function => "Функция",
            Elem::Operator => "Оператор",
            Elem::Special => "Особое (макрос, \\n)",
            Elem::Todo => "TODO / FIXME",
            Elem::LineNr => "Номер строки",
            Elem::CursorLineNr => "Номер тек. строки",
            Elem::CursorLine => "Текущая строка",
            Elem::Visual => "Выделение",
            Elem::Search => "Найденное",
            Elem::MatchParen => "Парная скобка",
            Elem::NonText => "Служебные знаки ~ »",
            Elem::ColorColumn => "Граница colorcolumn",
            Elem::ModeNormal => "Режим NORMAL",
            Elem::ModeInsert => "Режим INSERT",
            Elem::ModeVisual => "Режим VISUAL",
            Elem::ModeReplace => "Режим REPLACE",
            Elem::StatusLine => "Строка состояния",
            Elem::CmdLine => "Командная строка",
            Elem::ErrorMsg => "Сообщение об ошибке",
        }
    }

    /// Ключ в файле настроек.
    pub fn key(self) -> &'static str {
        match self {
            Elem::Normal => "normal",
            Elem::Comment => "comment",
            Elem::Keyword => "keyword",
            Elem::Type => "type",
            Elem::Constant => "constant",
            Elem::String => "string",
            Elem::Number => "number",
            Elem::Preproc => "preproc",
            Elem::Function => "function",
            Elem::Operator => "operator",
            Elem::Special => "special",
            Elem::Todo => "todo",
            Elem::LineNr => "linenr",
            Elem::CursorLineNr => "cursorlinenr",
            Elem::CursorLine => "cursorline",
            Elem::Visual => "visual",
            Elem::Search => "search",
            Elem::MatchParen => "matchparen",
            Elem::NonText => "nontext",
            Elem::ColorColumn => "colorcolumn",
            Elem::ModeNormal => "mode_normal",
            Elem::ModeInsert => "mode_insert",
            Elem::ModeVisual => "mode_visual",
            Elem::ModeReplace => "mode_replace",
            Elem::StatusLine => "statusline",
            Elem::CmdLine => "cmdline",
            Elem::ErrorMsg => "errormsg",
        }
    }

    /// Лексема подсветки (у неё фон обычно берётся от строки).
    pub fn is_syntax(self) -> bool {
        (self as usize) <= Elem::Todo as usize
    }
}

/// Названия 16 цветов BIOS.
pub const COLOR_NAMES: [&str; 16] = [
    "Чёрный",
    "Синий",
    "Зелёный",
    "Бирюзовый",
    "Красный",
    "Малиновый",
    "Коричневый",
    "Светло-серый",
    "Тёмно-серый",
    "Ярко-синий",
    "Ярко-зелёный",
    "Ярко-бирюзовый",
    "Ярко-красный",
    "Ярко-малиновый",
    "Жёлтый",
    "Белый",
];

/// Цвет элемента: текст и фон (0–15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub fg: u8,
    pub bg: u8,
}

impl Color {
    pub const fn new(fg: u8, bg: u8) -> Self {
        Color { fg, bg }
    }
    /// Атрибут tvision. Атрибут 0 в tvision значит «не менять цвет»,
    /// поэтому чёрное на чёрном заменяется тёмно-серым на чёрном.
    pub fn attr(self) -> Attr {
        let a = attr(self.fg, self.bg);
        if a == 0 {
            attr(8, 0)
        } else {
            a
        }
    }
}

/// Встроенная цветовая схема.
pub struct BuiltinScheme {
    pub name: &'static str,
    pub title: &'static str,
    pub colors: [Color; ELEM_COUNT],
}

const fn c(fg: u8, bg: u8) -> Color {
    Color::new(fg, bg)
}

/// Встроенные схемы. Порядок цветов — как в [`Elem::ALL`].
pub const SCHEMES: &[BuiltinScheme] = &[
    BuiltinScheme {
        name: "borland",
        title: "Borland (синяя)",
        colors: [
            c(14, 1), // Normal
            c(7, 1),  // Comment
            c(15, 1), // Keyword
            c(11, 1), // Type
            c(13, 1), // Constant
            c(10, 1), // String
            c(11, 1), // Number
            c(10, 1), // Preproc
            c(15, 1), // Function
            c(15, 1), // Operator
            c(13, 1), // Special
            c(0, 6),  // Todo
            c(3, 1),  // LineNr
            c(14, 1), // CursorLineNr
            c(14, 9), // CursorLine (используется фон)
            c(1, 7),  // Visual
            c(0, 3),  // Search
            c(15, 4), // MatchParen
            c(9, 1),  // NonText
            c(14, 9), // ColorColumn
            c(0, 2),  // ModeNormal
            c(15, 4), // ModeInsert
            c(0, 6),  // ModeVisual
            c(15, 5), // ModeReplace
            c(0, 3),  // StatusLine
            c(14, 1), // CmdLine
            c(15, 4), // ErrorMsg
        ],
    },
    BuiltinScheme {
        name: "dark",
        title: "Тёмная (vim)",
        colors: [
            c(7, 0),
            c(3, 0),
            c(14, 0),
            c(10, 0),
            c(13, 0),
            c(13, 0),
            c(13, 0),
            c(9, 0),
            c(11, 0),
            c(7, 0),
            c(12, 0),
            c(0, 14),
            c(14, 0),
            c(14, 0),
            c(7, 8),
            c(0, 7),
            c(0, 14),
            c(0, 3),
            c(9, 0),
            c(7, 4),
            c(0, 2),
            c(0, 11),
            c(0, 13),
            c(15, 4),
            c(15, 8),
            c(7, 0),
            c(15, 4),
        ],
    },
    BuiltinScheme {
        name: "light",
        title: "Светлая",
        colors: [
            c(0, 15),
            c(1, 15),
            c(4, 15),
            c(2, 15),
            c(5, 15),
            c(5, 15),
            c(5, 15),
            c(5, 15),
            c(3, 15),
            c(0, 15),
            c(6, 15),
            c(0, 14),
            c(6, 15),
            c(4, 15),
            c(0, 7),
            c(15, 1),
            c(0, 14),
            c(15, 3),
            c(9, 15),
            c(0, 7),
            c(15, 2),
            c(15, 1),
            c(15, 5),
            c(15, 4),
            c(15, 8),
            c(0, 15),
            c(15, 4),
        ],
    },
    BuiltinScheme {
        name: "matrix",
        title: "Матрица",
        colors: [
            c(2, 0),
            c(8, 0),
            c(10, 0),
            c(10, 0),
            c(15, 0),
            c(11, 0),
            c(11, 0),
            c(10, 0),
            c(15, 0),
            c(2, 0),
            c(14, 0),
            c(0, 10),
            c(8, 0),
            c(10, 0),
            c(10, 8),
            c(0, 2),
            c(0, 10),
            c(0, 11),
            c(8, 0),
            c(2, 8),
            c(0, 2),
            c(0, 10),
            c(0, 11),
            c(0, 14),
            c(10, 8),
            c(10, 0),
            c(15, 4),
        ],
    },
    BuiltinScheme {
        name: "desert",
        title: "Пустыня",
        colors: [
            c(15, 8),
            c(11, 8),
            c(14, 8),
            c(10, 8),
            c(12, 8),
            c(12, 8),
            c(12, 8),
            c(12, 8),
            c(10, 8),
            c(15, 8),
            c(14, 8),
            c(0, 14),
            c(14, 8),
            c(15, 8),
            c(15, 0),
            c(8, 14),
            c(0, 12),
            c(0, 11),
            c(9, 8),
            c(15, 0),
            c(0, 10),
            c(0, 14),
            c(0, 12),
            c(15, 4),
            c(0, 7),
            c(15, 8),
            c(15, 4),
        ],
    },
];

/// Схема по имени.
pub fn builtin_scheme(name: &str) -> Option<&'static BuiltinScheme> {
    SCHEMES.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

// ----------------------------------------------------------------------
// Параметры
// ----------------------------------------------------------------------

/// Параметры редактора (`:set`).
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub tabstop: usize,
    pub shiftwidth: usize,
    pub expandtab: bool,
    pub autoindent: bool,
    pub smartindent: bool,
    pub number: bool,
    pub relativenumber: bool,
    pub cursorline: bool,
    pub list: bool,
    pub syntax: bool,
    pub showmatch: bool,
    pub hlsearch: bool,
    pub incsearch: bool,
    pub ignorecase: bool,
    pub smartcase: bool,
    pub wrapscan: bool,
    /// Безымянный регистр связан с системным буфером обмена (`clipboard=unnamed`).
    pub clipboard: bool,
    /// Перед записью сохранять старую версию в `.bak`.
    pub backup: bool,
    /// Убирать пробелы в конце строк при записи.
    pub trimtrailing: bool,
    /// Перевод строки в конце файла.
    pub fixeol: bool,
    pub showcmd: bool,
    pub scrolloff: usize,
    /// Подсветить столбец (0 — нет).
    pub colorcolumn: usize,
    /// Клавиша `<Leader>`.
    pub leader: char,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            tabstop: 4,
            shiftwidth: 4,
            expandtab: true,
            autoindent: true,
            smartindent: true,
            number: true,
            relativenumber: false,
            cursorline: true,
            list: false,
            syntax: true,
            showmatch: true,
            hlsearch: true,
            incsearch: true,
            ignorecase: true,
            smartcase: true,
            wrapscan: true,
            clipboard: true,
            backup: false,
            trimtrailing: false,
            fixeol: true,
            showcmd: true,
            scrolloff: 3,
            colorcolumn: 0,
            leader: '\\',
        }
    }
}

/// Описание параметра для `:set`.
pub struct OptInfo {
    pub name: &'static str,
    pub short: &'static str,
    pub is_bool: bool,
}

pub const OPTIONS: &[OptInfo] = &[
    OptInfo { name: "tabstop", short: "ts", is_bool: false },
    OptInfo { name: "shiftwidth", short: "sw", is_bool: false },
    OptInfo { name: "expandtab", short: "et", is_bool: true },
    OptInfo { name: "autoindent", short: "ai", is_bool: true },
    OptInfo { name: "smartindent", short: "si", is_bool: true },
    OptInfo { name: "number", short: "nu", is_bool: true },
    OptInfo { name: "relativenumber", short: "rnu", is_bool: true },
    OptInfo { name: "cursorline", short: "cul", is_bool: true },
    OptInfo { name: "list", short: "list", is_bool: true },
    OptInfo { name: "syntax", short: "syn", is_bool: true },
    OptInfo { name: "showmatch", short: "sm", is_bool: true },
    OptInfo { name: "hlsearch", short: "hls", is_bool: true },
    OptInfo { name: "incsearch", short: "is", is_bool: true },
    OptInfo { name: "ignorecase", short: "ic", is_bool: true },
    OptInfo { name: "smartcase", short: "scs", is_bool: true },
    OptInfo { name: "wrapscan", short: "ws", is_bool: true },
    OptInfo { name: "clipboard", short: "cb", is_bool: true },
    OptInfo { name: "backup", short: "bk", is_bool: true },
    OptInfo { name: "trimtrailing", short: "tt", is_bool: true },
    OptInfo { name: "fixeol", short: "fixendofline", is_bool: true },
    OptInfo { name: "showcmd", short: "sc", is_bool: true },
    OptInfo { name: "scrolloff", short: "so", is_bool: false },
    OptInfo { name: "colorcolumn", short: "cc", is_bool: false },
];

/// Найти параметр по полному или короткому имени.
pub fn find_option(name: &str) -> Option<&'static OptInfo> {
    OPTIONS.iter().find(|o| o.name == name || o.short == name)
}

impl Options {
    /// Логический параметр.
    pub fn bool_mut(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "expandtab" => &mut self.expandtab,
            "autoindent" => &mut self.autoindent,
            "smartindent" => &mut self.smartindent,
            "number" => &mut self.number,
            "relativenumber" => &mut self.relativenumber,
            "cursorline" => &mut self.cursorline,
            "list" => &mut self.list,
            "syntax" => &mut self.syntax,
            "showmatch" => &mut self.showmatch,
            "hlsearch" => &mut self.hlsearch,
            "incsearch" => &mut self.incsearch,
            "ignorecase" => &mut self.ignorecase,
            "smartcase" => &mut self.smartcase,
            "wrapscan" => &mut self.wrapscan,
            "clipboard" => &mut self.clipboard,
            "backup" => &mut self.backup,
            "trimtrailing" => &mut self.trimtrailing,
            "fixeol" => &mut self.fixeol,
            "showcmd" => &mut self.showcmd,
            _ => return None,
        })
    }

    /// Числовой параметр.
    pub fn num_mut(&mut self, name: &str) -> Option<&mut usize> {
        Some(match name {
            "tabstop" => &mut self.tabstop,
            "shiftwidth" => &mut self.shiftwidth,
            "scrolloff" => &mut self.scrolloff,
            "colorcolumn" => &mut self.colorcolumn,
            _ => return None,
        })
    }

    /// Привести значения к допустимым.
    pub fn sanitize(&mut self) {
        self.tabstop = self.tabstop.clamp(1, 32);
        self.shiftwidth = self.shiftwidth.clamp(1, 32);
        self.scrolloff = self.scrolloff.min(50);
        self.colorcolumn = self.colorcolumn.min(500);
    }

    /// Текст `:set` для одного параметра: `number`, `nonumber`, `tabstop=4`.
    pub fn show(&mut self, name: &str) -> String {
        if let Some(b) = self.bool_mut(name) {
            if *b {
                format!("  {}", name)
            } else {
                format!("no{}", name)
            }
        } else if let Some(n) = self.num_mut(name) {
            format!("  {}={}", name, n)
        } else {
            String::new()
        }
    }
}

// ----------------------------------------------------------------------
// Сопоставления клавиш
// ----------------------------------------------------------------------

/// Режим, в котором действует сопоставление.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapMode {
    Normal,
    Insert,
    Visual,
}

impl MapMode {
    pub const ALL: [MapMode; 3] = [MapMode::Normal, MapMode::Insert, MapMode::Visual];
    pub fn key(self) -> &'static str {
        match self {
            MapMode::Normal => "n",
            MapMode::Insert => "i",
            MapMode::Visual => "v",
        }
    }
    pub fn from_key(s: &str) -> Option<MapMode> {
        match s {
            "n" => Some(MapMode::Normal),
            "i" => Some(MapMode::Insert),
            "v" | "x" => Some(MapMode::Visual),
            _ => None,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            MapMode::Normal => "Обычный",
            MapMode::Insert => "Вставка",
            MapMode::Visual => "Визуальный",
        }
    }
}

/// Сопоставление `lhs` → `rhs` (без рекурсии, как `:noremap`).
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    pub mode: MapMode,
    pub lhs: String,
    pub rhs: String,
}

impl Mapping {
    /// Клавиши обеих частей; `None` — запись с ошибкой.
    pub fn keys(&self, leader: char) -> Option<(Vec<VKey>, Vec<VKey>)> {
        let l = parse_keys(&self.lhs, leader).ok()?;
        let r = parse_keys(&self.rhs, leader).ok()?;
        if l.is_empty() {
            None
        } else {
            Some((l, r))
        }
    }
}

/// Сопоставления по умолчанию (примеры, которые можно удалить в диалоге).
fn default_mappings() -> Vec<Mapping> {
    vec![
        Mapping { mode: MapMode::Normal, lhs: "<Leader>w".into(), rhs: ":w<CR>".into() },
        Mapping { mode: MapMode::Normal, lhs: "<Leader>q".into(), rhs: ":q<CR>".into() },
        Mapping { mode: MapMode::Normal, lhs: "<F4>".into(), rhs: ":noh<CR>".into() },
    ]
}

// ----------------------------------------------------------------------
// Все настройки
// ----------------------------------------------------------------------

/// Тип файла: язык подсветки и расширения/имена файлов.
#[derive(Clone, Debug, PartialEq)]
pub struct FileType {
    /// Идентификатор языка из [`LANGS`].
    pub lang: String,
    /// Расширения и имена через пробел или `;`: `rs`, `Makefile`, `*.mk`.
    pub patterns: String,
    /// Свои tabstop/shiftwidth и expandtab для этого типа (0 — как везде).
    pub tabstop: usize,
    pub expandtab: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub options: Options,
    /// Имя схемы, из которой взяты цвета (или «custom»).
    pub scheme: String,
    pub colors: Vec<Color>,
    pub filetypes: Vec<FileType>,
    pub mappings: Vec<Mapping>,
    /// Растёт при каждом изменении (окна сбрасывают кэши).
    pub generation: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            options: Options::default(),
            scheme: SCHEMES[0].name.into(),
            colors: SCHEMES[0].colors.to_vec(),
            filetypes: default_filetypes(),
            mappings: default_mappings(),
            generation: 1,
        }
    }
}

/// Типы файлов по умолчанию — из описаний языков.
pub fn default_filetypes() -> Vec<FileType> {
    LANGS
        .iter()
        .map(|l| FileType {
            lang: l.id.into(),
            patterns: l.patterns.into(),
            tabstop: match l.id {
                "make" => 8,
                "yaml" | "json" | "html" | "xml" | "css" | "javascript" | "typescript" | "ruby" | "lua" => 2,
                _ => 0,
            },
            expandtab: if l.id == "make" || l.id == "go" { Some(false) } else { None },
        })
        .collect()
}

impl Config {
    pub fn shared(self) -> Shared {
        Rc::new(RefCell::new(self))
    }

    /// Цвет элемента.
    pub fn color(&self, e: Elem) -> Color {
        self.colors.get(e as usize).copied().unwrap_or(Color::new(7, 0))
    }

    pub fn attr(&self, e: Elem) -> Attr {
        self.color(e).attr()
    }

    /// Загрузить встроенную схему.
    pub fn load_scheme(&mut self, name: &str) -> bool {
        match builtin_scheme(name) {
            Some(s) => {
                self.scheme = s.name.into();
                self.colors = s.colors.to_vec();
                self.touch();
                true
            }
            None => false,
        }
    }

    /// Отметить изменение.
    pub fn touch(&mut self) {
        self.generation += 1;
    }

    /// Тип файла по имени: сначала шаблоны из настроек, потом первая строка
    /// (`#!/bin/sh`, `<?xml`).
    pub fn detect_lang(&self, path: &std::path::Path, first_line: &str) -> Option<String> {
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = path.extension().map(|s| s.to_string_lossy().to_lowercase());
        for ft in &self.filetypes {
            for p in ft.patterns.split(|c: char| c == ';' || c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()) {
                let p = p.trim_start_matches("*.").trim_start_matches('.');
                let hit = if p.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) || p.contains('.') {
                    // Имя файла целиком: Makefile, CMakeLists.txt.
                    name.eq_ignore_ascii_case(p)
                } else {
                    ext.as_deref() == Some(&p.to_lowercase()) || name.eq_ignore_ascii_case(p)
                };
                if hit {
                    return Some(ft.lang.clone());
                }
            }
        }
        let f = first_line.trim();
        if let Some(sh) = f.strip_prefix("#!") {
            let sh = sh.to_lowercase();
            for (needle, lang) in [("python", "python"), ("node", "javascript"), ("ruby", "ruby"), ("lua", "lua"), ("php", "php"), ("sh", "shell")] {
                if sh.contains(needle) {
                    return Some(lang.into());
                }
            }
        }
        if f.starts_with("<?xml") {
            return Some("xml".into());
        }
        if f.to_lowercase().starts_with("<!doctype html") {
            return Some("html".into());
        }
        None
    }

    /// Табуляция и expandtab для языка с учётом настроек типа файла.
    pub fn indent_for(&self, lang: Option<&str>) -> (usize, usize, bool) {
        let o = &self.options;
        let mut ts = o.tabstop;
        let mut sw = o.shiftwidth;
        let mut et = o.expandtab;
        if let Some(ft) = lang.and_then(|l| self.filetypes.iter().find(|f| f.lang == l)) {
            if ft.tabstop > 0 {
                ts = ft.tabstop;
                sw = ft.tabstop;
            }
            if let Some(e) = ft.expandtab {
                et = e;
            }
        }
        (ts, sw, et)
    }

    /// Сопоставления режима в виде клавиш.
    pub fn compiled_mappings(&self, mode: MapMode) -> Vec<(Vec<VKey>, Vec<VKey>)> {
        self.mappings.iter().filter(|m| m.mode == mode).filter_map(|m| m.keys(self.options.leader)).collect()
    }

    // ---------- JSON ----------

    pub fn to_json(&self) -> Json {
        let mut opts: Vec<(&str, Json)> = Vec::new();
        let mut o = self.options.clone();
        for info in OPTIONS {
            if info.is_bool {
                opts.push((info.name, Json::Bool(*o.bool_mut(info.name).unwrap())));
            } else {
                opts.push((info.name, Json::from_i64(*o.num_mut(info.name).unwrap() as i64)));
            }
        }
        opts.push(("leader", Json::from_str(&o.leader.to_string())));
        let colors = Elem::ALL
            .iter()
            .map(|e| {
                let c = self.color(*e);
                (e.key(), Json::Arr(vec![Json::from_i64(c.fg as i64), Json::from_i64(c.bg as i64)]))
            })
            .collect();
        let fts = self
            .filetypes
            .iter()
            .map(|f| {
                let mut v =
                    vec![("lang", Json::from_str(&f.lang)), ("patterns", Json::from_str(&f.patterns)), ("tabstop", Json::from_i64(f.tabstop as i64))];
                if let Some(e) = f.expandtab {
                    v.push(("expandtab", Json::Bool(e)));
                }
                Json::object(v)
            })
            .collect();
        let maps = self
            .mappings
            .iter()
            .map(|m| Json::object(vec![("mode", Json::from_str(m.mode.key())), ("lhs", Json::from_str(&m.lhs)), ("rhs", Json::from_str(&m.rhs))]))
            .collect();
        Json::object(vec![
            ("options", Json::object(opts)),
            ("scheme", Json::from_str(&self.scheme)),
            ("colors", Json::object(colors)),
            ("filetypes", Json::Arr(fts)),
            ("mappings", Json::Arr(maps)),
        ])
    }

    /// Из JSON; чего нет — по умолчанию.
    pub fn from_json(j: &Json) -> Config {
        let mut cfg = Config::default();
        if let Some(o) = j.get("options") {
            for info in OPTIONS {
                if let Some(v) = o.get(info.name) {
                    if info.is_bool {
                        if let (Some(b), Some(dst)) = (v.as_bool(), cfg.options.bool_mut(info.name)) {
                            *dst = b;
                        }
                    } else if let (Some(n), Some(dst)) = (v.as_i64(), cfg.options.num_mut(info.name)) {
                        *dst = n.max(0) as usize;
                    }
                }
            }
            if let Some(l) = o.get("leader").and_then(Json::as_str).and_then(|s| s.chars().next()) {
                cfg.options.leader = l;
            }
            cfg.options.sanitize();
        }
        if let Some(s) = j.get("scheme").and_then(Json::as_str) {
            cfg.load_scheme(s);
            cfg.scheme = s.into();
        }
        if let Some(cols) = j.get("colors") {
            for e in Elem::ALL {
                if let Some(arr) = cols.get(e.key()).and_then(Json::as_array) {
                    if let (Some(fg), Some(bg)) = (arr.first().and_then(Json::as_i64), arr.get(1).and_then(Json::as_i64)) {
                        cfg.colors[e as usize] = Color::new((fg & 15) as u8, (bg & 15) as u8);
                    }
                }
            }
        }
        if let Some(fts) = j.get("filetypes").and_then(Json::as_array) {
            let mut list: Vec<FileType> = fts
                .iter()
                .filter_map(|f| {
                    Some(FileType {
                        lang: f.get("lang")?.as_str()?.to_string(),
                        patterns: f.get("patterns").and_then(Json::as_str).unwrap_or("").to_string(),
                        tabstop: f.get("tabstop").and_then(Json::as_i64).unwrap_or(0).clamp(0, 32) as usize,
                        expandtab: f.get("expandtab").and_then(Json::as_bool),
                    })
                })
                .collect();
            // Языки, которых не было в файле (новые версии редактора), — по умолчанию.
            for d in default_filetypes() {
                if !list.iter().any(|f| f.lang == d.lang) {
                    list.push(d);
                }
            }
            list.retain(|f| LANGS.iter().any(|l| l.id == f.lang));
            cfg.filetypes = list;
        }
        if let Some(ms) = j.get("mappings").and_then(Json::as_array) {
            cfg.mappings = ms
                .iter()
                .filter_map(|m| {
                    Some(Mapping {
                        mode: MapMode::from_key(m.get("mode")?.as_str()?)?,
                        lhs: m.get("lhs")?.as_str()?.to_string(),
                        rhs: m.get("rhs").and_then(Json::as_str).unwrap_or("").to_string(),
                    })
                })
                .collect();
        }
        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let mut c = Config::default();
        c.options.tabstop = 8;
        c.options.number = false;
        c.load_scheme("dark");
        c.colors[Elem::Keyword as usize] = Color::new(12, 0);
        c.mappings.push(Mapping { mode: MapMode::Insert, lhs: "jk".into(), rhs: "<Esc>".into() });
        let j = c.to_json();
        let back = Config::from_json(&tvision::json::parse_json(&j.pretty()).unwrap());
        assert_eq!(back.options, c.options);
        assert_eq!(back.colors, c.colors);
        assert_eq!(back.mappings, c.mappings);
        assert_eq!(back.filetypes, c.filetypes);
        assert_eq!(back.scheme, "dark");
    }

    #[test]
    fn detect() {
        let c = Config::default();
        let d = |p: &str, l: &str| c.detect_lang(std::path::Path::new(p), l);
        assert_eq!(d("main.rs", "").as_deref(), Some("rust"));
        assert_eq!(d("/x/Makefile", "").as_deref(), Some("make"));
        assert_eq!(d("script", "#!/usr/bin/env python3").as_deref(), Some("python"));
        assert_eq!(d("a.HPP", "").as_deref(), Some("cpp"));
        assert_eq!(d("notes.txt", "").as_deref(), None);
    }

    #[test]
    fn schemes_complete() {
        assert_eq!(Elem::ALL.len(), ELEM_COUNT);
        for (i, e) in Elem::ALL.iter().enumerate() {
            assert_eq!(*e as usize, i);
        }
    }
}
