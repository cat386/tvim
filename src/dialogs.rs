//! Диалоги настроек.
//!
//! * [`options_dialog`] — параметры редактора (то же, что `:set`);
//! * [`colors_dialog`] — цветовая схема: готовые схемы и цвет каждого
//!   элемента, с живым образцом;
//! * [`syntax_dialog`] — типы файлов: какие файлы каким языком подсвечивать,
//!   своя табуляция для языка;
//! * [`filetype_dialog`] — язык подсветки текущего окна;
//! * [`keymap_dialog`] — сопоставления клавиш (`:map`).
//!
//! Диалоги, которым нужно реагировать на кнопки и выбор в списках, —
//! это [`HookDialog`]: обычный [`Dialog`] плюс обработчики команд и
//! «после каждого события».

use std::cell::RefCell;
use std::rc::Rc;

use tvision::*;

use crate::config::{default_filetypes, Color, Config, Elem, MapMode, Mapping, Options, Shared, COLOR_NAMES, SCHEMES};
use crate::keys::parse_keys;
use crate::syntax::{highlight, lang_by_id, LState, LANGS};

/// Диалог с обработчиками.
pub struct HookDialog {
    pub dlg: Dialog,
    /// Команда от кнопки: `true` — обработана.
    pub on_command: Option<Box<dyn FnMut(&mut Dialog, u16, &mut Ctx) -> bool>>,
    /// Вызывается после каждого события (синхронизация полей).
    pub after: Option<Box<dyn FnMut(&mut Dialog, &mut Ctx)>>,
}

impl HookDialog {
    pub fn new(dlg: Dialog) -> Self {
        HookDialog { dlg, on_command: None, after: None }
    }
}

impl View for HookDialog {
    tvision::impl_group_basics!(dlg.win.group);

    fn as_window(&self) -> Option<&Window> {
        Some(&self.dlg.win)
    }

    fn as_window_mut(&mut self) -> Option<&mut Window> {
        Some(&mut self.dlg.win)
    }

    fn draw(&self, dc: &mut DrawCtx) {
        self.dlg.draw(dc);
    }

    fn handle_event(&mut self, ev: &mut Event, ctx: &mut Ctx) {
        if let Event::Command(m) = ev {
            let c = m.command;
            if let Some(f) = &mut self.on_command {
                if f(&mut self.dlg, c, ctx) {
                    ev.clear();
                }
            }
        }
        if !ev.is_nothing() {
            self.dlg.handle_event(ev, ctx);
        }
        if let Some(f) = &mut self.after {
            f(&mut self.dlg, ctx);
        }
    }

    fn palette(&self) -> &'static [u8] {
        self.dlg.palette()
    }

    fn set_state(&mut self, flag: u16, enable: bool) {
        self.dlg.set_state(flag, enable);
    }

    fn valid(&mut self, command: u16, ctx: &mut Ctx) -> bool {
        self.dlg.valid(command, ctx)
    }

    fn size_limits(&self, owner_size: Point) -> (Point, Point) {
        self.dlg.size_limits(owner_size)
    }
}

fn cluster<'a>(d: &'a Dialog, id: ViewId) -> &'a Cluster {
    d.win.group.find::<Cluster>(id).expect("флажки")
}

fn cluster_mut<'a>(d: &'a mut Dialog, id: ViewId) -> &'a mut Cluster {
    d.win.group.find_mut::<Cluster>(id).expect("флажки")
}

fn input<'a>(d: &'a Dialog, id: ViewId) -> &'a InputLine {
    d.win.group.find::<InputLine>(id).expect("строка ввода")
}

fn input_mut<'a>(d: &'a mut Dialog, id: ViewId) -> &'a mut InputLine {
    d.win.group.find_mut::<InputLine>(id).expect("строка ввода")
}

fn list<'a>(d: &'a Dialog, id: ViewId) -> &'a ListBox {
    d.win.group.find::<ListBox>(id).expect("список")
}

fn list_mut<'a>(d: &'a mut Dialog, id: ViewId) -> &'a mut ListBox {
    d.win.group.find_mut::<ListBox>(id).expect("список")
}

/// Список с вертикальной полосой прокрутки справа.
fn add_list(d: &mut Dialog, r: Rect, items: Vec<String>) -> ViewId {
    let sb = d.add(ScrollBar::new(Rect::new(r.b.x, r.a.y, r.b.x + 1, r.b.y)));
    let mut lb = ListBox::new(r, 1, Some(sb));
    lb.set_items(items);
    d.add(lb)
}

fn num_input(d: &mut Dialog, x: i32, y: i32, label: &str, label_x: i32, value: usize, max: i64) -> ViewId {
    let mut il = InputLine::new(Rect::new(x, y, x + 7, y + 1), 4).with_validator(RangeValidator::new(0, max));
    il.set_text(&value.to_string());
    let id = d.add(il);
    d.add(Label::new(Rect::new(label_x, y, x - 1, y + 1), label, Some(id)));
    id
}

// ----------------------------------------------------------------------
// Параметры редактора
// ----------------------------------------------------------------------

const CM_DEFAULTS: u16 = 400;
const CM_LOAD_SCHEME: u16 = 401;
const CM_ADD: u16 = 402;
const CM_CHANGE: u16 = 403;
const CM_DELETE: u16 = 404;
const CM_RESET: u16 = 405;

const SHOW_OPTS: [(&str, &str); 8] = [
    ("Номера строк", "number"),
    ("Относительные номера", "relativenumber"),
    ("Подсвечивать тек. строку", "cursorline"),
    ("Показывать табуляции »·", "list"),
    ("Подсветка синтаксиса", "syntax"),
    ("Парные скобки", "showmatch"),
    ("Подсвечивать найденное", "hlsearch"),
    ("Набираемая команда", "showcmd"),
];
const EDIT_OPTS: [(&str, &str); 7] = [
    ("Пробелы вместо табуляции", "expandtab"),
    ("Автоотступ", "autoindent"),
    ("Умный отступ ({, :)", "smartindent"),
    ("Общий буфер обмена", "clipboard"),
    ("Резервная копия .bak", "backup"),
    ("Убирать пробелы в конце", "trimtrailing"),
    ("Перевод строки в конце", "fixeol"),
];
const SEARCH_OPTS: [(&str, &str); 4] = [
    ("Без учёта регистра", "ignorecase"),
    ("Умный регистр (smartcase)", "smartcase"),
    ("Пошаговый поиск", "incsearch"),
    ("Поиск по кругу", "wrapscan"),
];

fn mask_of(o: &mut Options, items: &[(&str, &str)]) -> u32 {
    let mut m = 0;
    for (i, (_, name)) in items.iter().enumerate() {
        if *o.bool_mut(name).unwrap() {
            m |= 1 << i;
        }
    }
    m
}

fn apply_mask(o: &mut Options, items: &[(&str, &str)], m: u32) {
    for (i, (_, name)) in items.iter().enumerate() {
        *o.bool_mut(name).unwrap() = m & (1 << i) != 0;
    }
}

/// Диалог «Параметры редактора». `true` — нажали OK.
pub fn options_dialog(app: &mut Program, cfg: &Shared) -> bool {
    let mut o = cfg.borrow().options.clone();
    let mut d = Dialog::centered(76, 22, "Параметры редактора");

    fn names<'a>(items: &[(&'a str, &str)]) -> Vec<&'a str> {
        items.iter().map(|(t, _)| *t).collect()
    }
    let mut show = CheckBoxes::check_boxes(Rect::new(2, 2, 36, 10), &names(&SHOW_OPTS));
    show.value = mask_of(&mut o, &SHOW_OPTS);
    let show_id = d.add(show);
    d.add(Label::new(Rect::new(2, 1, 20, 2), "~О~тображение", Some(show_id)));

    let mut edit = CheckBoxes::check_boxes(Rect::new(38, 2, 74, 9), &names(&EDIT_OPTS));
    edit.value = mask_of(&mut o, &EDIT_OPTS);
    let edit_id = d.add(edit);
    d.add(Label::new(Rect::new(38, 1, 50, 2), "~П~равка", Some(edit_id)));

    let mut search = CheckBoxes::check_boxes(Rect::new(2, 12, 36, 16), &names(&SEARCH_OPTS));
    search.value = mask_of(&mut o, &SEARCH_OPTS);
    let search_id = d.add(search);
    d.add(Label::new(Rect::new(2, 11, 12, 12), "По~и~ск", Some(search_id)));

    let ts_id = num_input(&mut d, 66, 11, "~Т~абуляция:", 38, o.tabstop, 32);
    let sw_id = num_input(&mut d, 66, 12, "~Ш~ирина отступа:", 38, o.shiftwidth, 32);
    let so_id = num_input(&mut d, 66, 13, "Поля про~к~рутки:", 38, o.scrolloff, 50);
    let cc_id = num_input(&mut d, 66, 14, "~Г~раница (столбец):", 38, o.colorcolumn, 500);
    let mut leader = InputLine::new(Rect::new(66, 15, 73, 16), 1);
    leader.set_text(&o.leader.to_string());
    let leader_id = d.add(leader);
    d.add(Label::new(Rect::new(38, 15, 65, 16), "Клавиша <~L~eader>:", Some(leader_id)));

    d.add(StaticText::new(Rect::new(2, 17, 74, 18), "\x03Те же параметры меняет команда :set (например, :set nonumber ts=8)"));
    d.add(Button::new(Rect::new(14, 19, 26, 21), "O~K~", cm::OK, bf::DEFAULT));
    d.add(Button::new(Rect::new(28, 19, 40, 21), "Отмена", cm::CANCEL, bf::NORMAL));
    d.add(Button::new(Rect::new(42, 19, 62, 21), "По ~у~молчанию", CM_DEFAULTS, bf::NORMAL));
    d.select(show_id);

    let mut h = HookDialog::new(d);
    h.on_command = Some(Box::new(move |d, c, _ctx| {
        if c != CM_DEFAULTS {
            return false;
        }
        let mut def = Options::default();
        cluster_mut(d, show_id).value = mask_of(&mut def, &SHOW_OPTS);
        cluster_mut(d, edit_id).value = mask_of(&mut def, &EDIT_OPTS);
        cluster_mut(d, search_id).value = mask_of(&mut def, &SEARCH_OPTS);
        input_mut(d, ts_id).set_text(&def.tabstop.to_string());
        input_mut(d, sw_id).set_text(&def.shiftwidth.to_string());
        input_mut(d, so_id).set_text(&def.scrolloff.to_string());
        input_mut(d, cc_id).set_text(&def.colorcolumn.to_string());
        input_mut(d, leader_id).set_text(&def.leader.to_string());
        true
    }));
    let (cmd, h) = app.exec_dialog(h);
    if cmd != cm::OK {
        return false;
    }
    let d = &h.dlg;
    apply_mask(&mut o, &SHOW_OPTS, cluster(d, show_id).value);
    apply_mask(&mut o, &EDIT_OPTS, cluster(d, edit_id).value);
    apply_mask(&mut o, &SEARCH_OPTS, cluster(d, search_id).value);
    let num = |id: ViewId, def: usize| input(d, id).text().trim().parse::<usize>().unwrap_or(def);
    o.tabstop = num(ts_id, o.tabstop);
    o.shiftwidth = num(sw_id, o.shiftwidth);
    o.scrolloff = num(so_id, o.scrolloff);
    o.colorcolumn = num(cc_id, o.colorcolumn);
    if let Some(c) = input(d, leader_id).text().chars().next() {
        o.leader = c;
    }
    o.sanitize();
    let mut c = cfg.borrow_mut();
    c.options = o;
    c.touch();
    true
}

// ----------------------------------------------------------------------
// Цвета
// ----------------------------------------------------------------------

/// Образец текста в цветах редактируемой схемы.
pub struct SchemePreview {
    pub vs: ViewState,
    pub colors: Rc<RefCell<Vec<Color>>>,
}

const SAMPLE: [&str; 6] =
    ["// Образец. TODO: проверить", "fn main() {", "    let n: u32 = 0x2A;", "    println!(\"Привет, {}!\\n\", n);", "    if n > 0 { return; }", "}"];

impl View for SchemePreview {
    tvision::impl_view_basics!();

    fn draw(&self, dc: &mut DrawCtx) {
        let colors = self.colors.borrow();
        let col = |e: Elem| colors.get(e as usize).copied().unwrap_or(Color::new(7, 0));
        let w = self.vs.size.x.max(1) as usize;
        let rust = lang_by_id("rust").unwrap();
        let normal = col(Elem::Normal);
        let mut st = LState::Normal;
        let mut toks = Vec::new();
        for (row, line) in SAMPLE.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            st = highlight(rust, &chars, st, &mut toks);
            let is_cur = row == 2;
            let line_bg = if is_cur { col(Elem::CursorLine).bg } else { normal.bg };
            let mut b = DrawBuffer::new(w);
            b.move_char(0, ' ', Color::new(normal.fg, line_bg).attr(), w);
            let nr = if is_cur { col(Elem::CursorLineNr) } else { col(Elem::LineNr) };
            b.move_str(0, &format!("{:>3} ", row + 1), nr.attr());
            for (i, &c) in chars.iter().enumerate() {
                let x = 4 + i;
                if x >= w {
                    break;
                }
                let t = col(toks[i]);
                let bg = if t.bg != normal.bg { t.bg } else { line_bg };
                let mut a = Color::new(t.fg, bg).attr();
                // Найденное «Привет», выделение «return;», парные скобки строки 5.
                if row == 3 && (14..20).contains(&i) {
                    a = col(Elem::Search).attr();
                }
                if row == 4 && (15..22).contains(&i) {
                    a = col(Elem::Visual).attr();
                }
                if row == 4 && (i == 13 || i == 23) {
                    a = col(Elem::MatchParen).attr();
                }
                let mut tmp = [0u8; 4];
                b.move_str(x, c.encode_utf8(&mut tmp), a);
            }
            if row == 2 && w > 50 {
                b.move_char(4 + 45, ' ', Color::new(normal.fg, col(Elem::ColorColumn).bg).attr(), 1);
            }
            dc.write_line(0, row as i32, w as i32, 1, &b);
        }
        let mut row = SAMPLE.len() as i32;
        if row < self.vs.size.y - 2 {
            let mut b = DrawBuffer::new(w);
            b.move_char(0, ' ', normal.attr(), w);
            b.move_char(0, '~', col(Elem::NonText).attr(), 1);
            dc.write_line(0, row, w as i32, 1, &b);
            row += 1;
        }
        // Строка состояния и командная строка.
        let mut b = DrawBuffer::new(w);
        b.move_char(0, ' ', col(Elem::StatusLine).attr(), w);
        let x = b.move_str(0, " NORMAL ", col(Elem::ModeNormal).attr());
        let x = x + b.move_str(x, " INSERT ", col(Elem::ModeInsert).attr());
        let x = x + b.move_str(x, " VISUAL ", col(Elem::ModeVisual).attr());
        b.move_str(x, " main.rs [+]", col(Elem::StatusLine).attr());
        dc.write_line(0, row, w as i32, 1, &b);
        let mut b = DrawBuffer::new(w);
        b.move_char(0, ' ', col(Elem::CmdLine).attr(), w);
        let x = b.move_str(0, ":w ", col(Elem::CmdLine).attr());
        b.move_str(x, "E37: Изменения не сохранены", col(Elem::ErrorMsg).attr());
        dc.write_line(0, row + 1, w as i32, 1, &b);
        for y in row + 2..self.vs.size.y {
            let mut b = DrawBuffer::new(w);
            b.move_char(0, ' ', normal.attr(), w);
            dc.write_line(0, y, w as i32, 1, &b);
        }
    }
}

/// Диалог «Цветовая схема».
pub fn colors_dialog(app: &mut Program, cfg: &Shared) -> bool {
    let working = Rc::new(RefCell::new(cfg.borrow().colors.clone()));
    let scheme_name = Rc::new(RefCell::new(cfg.borrow().scheme.clone()));
    let mut d = Dialog::centered(78, 23, "Цветовая схема");

    let elems: Vec<String> = Elem::ALL.iter().map(|e| e.title().to_string()).collect();
    let elem_id = add_list(&mut d, Rect::new(2, 2, 27, 19), elems);
    d.add(Label::new(Rect::new(2, 1, 20, 2), "~Э~лемент", Some(elem_id)));

    let fg_id = d.add(ColorSelector::new(Rect::new(30, 2, 42, 6), ColorSel::Foreground));
    d.add(Label::new(Rect::new(30, 1, 42, 2), "~Т~екст", Some(fg_id)));
    let bg_id = d.add(ColorSelector::new(Rect::new(44, 2, 56, 6), ColorSel::Foreground));
    d.add(Label::new(Rect::new(44, 1, 56, 2), "~Ф~он", Some(bg_id)));
    let names_id = d.add(StaticText::new(Rect::new(30, 7, 57, 10), ""));

    let schemes: Vec<String> = SCHEMES.iter().map(|s| s.title.to_string()).collect();
    let mut slb = ListBox::new(Rect::new(58, 2, 76, 7), 1, None);
    slb.set_items(schemes);
    if let Some(i) = SCHEMES.iter().position(|s| s.name == cfg.borrow().scheme) {
        slb.focus_item(i);
    }
    let schemes_id = d.add(slb);
    d.add(Label::new(Rect::new(58, 1, 76, 2), "~Г~отовые схемы", Some(schemes_id)));
    d.add(Button::new(Rect::new(58, 8, 76, 10), "~З~агрузить", CM_LOAD_SCHEME, bf::NORMAL));

    d.add(StaticText::new(Rect::new(30, 10, 76, 11), "Образец:"));
    d.add(SchemePreview { vs: ViewState::new(Rect::new(30, 11, 76, 20)), colors: working.clone() });
    d.add(Button::new(Rect::new(40, 20, 52, 22), "O~K~", cm::OK, bf::DEFAULT));
    d.add(Button::new(Rect::new(54, 20, 66, 22), "Отмена", cm::CANCEL, bf::NORMAL));
    d.select(elem_id);

    // Начальные цвета выбранного элемента.
    {
        let c = working.borrow()[0];
        d.win.group.find_mut::<ColorSelector>(fg_id).unwrap().color = c.fg;
        d.win.group.find_mut::<ColorSelector>(bg_id).unwrap().color = c.bg;
    }

    let mut h = HookDialog::new(d);
    let w2 = working.clone();
    let sn = scheme_name.clone();
    h.on_command = Some(Box::new(move |d, c, _ctx| {
        if c != CM_LOAD_SCHEME {
            return false;
        }
        let i = list(d, schemes_id).focused;
        if let Some(s) = SCHEMES.get(i) {
            *w2.borrow_mut() = s.colors.to_vec();
            *sn.borrow_mut() = s.name.to_string();
            let e = list(d, elem_id).focused;
            let col = w2.borrow()[e];
            d.win.group.find_mut::<ColorSelector>(fg_id).unwrap().color = col.fg;
            d.win.group.find_mut::<ColorSelector>(bg_id).unwrap().color = col.bg;
        }
        true
    }));
    let w3 = working.clone();
    let sn2 = scheme_name.clone();
    let mut last_elem = 0usize;
    h.after = Some(Box::new(move |d, _ctx| {
        let e = list(d, elem_id).focused.min(Elem::ALL.len() - 1);
        if e != last_elem {
            last_elem = e;
            let c = w3.borrow()[e];
            d.win.group.find_mut::<ColorSelector>(fg_id).unwrap().color = c.fg;
            d.win.group.find_mut::<ColorSelector>(bg_id).unwrap().color = c.bg;
        } else {
            let fg = d.win.group.find::<ColorSelector>(fg_id).unwrap().color;
            let bg = d.win.group.find::<ColorSelector>(bg_id).unwrap().color;
            let mut w = w3.borrow_mut();
            if w[e] != Color::new(fg, bg) {
                w[e] = Color::new(fg, bg);
                *sn2.borrow_mut() = "custom".into();
            }
        }
        let c = w3.borrow()[e];
        let hint = if Elem::ALL[e].is_syntax() && e != 0 {
            format!("Текст: {}\nФон: {}\n(обычный фон = фон строки)", COLOR_NAMES[c.fg as usize], COLOR_NAMES[c.bg as usize])
        } else if Elem::ALL[e] == Elem::CursorLine || Elem::ALL[e] == Elem::ColorColumn {
            format!("Фон: {}\n(важен только фон)", COLOR_NAMES[c.bg as usize])
        } else {
            format!("Текст: {}\nФон: {}", COLOR_NAMES[c.fg as usize], COLOR_NAMES[c.bg as usize])
        };
        if let Some(t) = d.win.group.find_mut::<StaticText>(names_id) {
            t.text = hint;
        }
    }));
    let (cmd, _h) = app.exec_dialog(h);
    if cmd != cm::OK {
        return false;
    }
    let mut c = cfg.borrow_mut();
    c.colors = working.borrow().clone();
    let name = scheme_name.borrow().clone();
    // Схема без изменений сохраняет своё имя.
    c.scheme = match SCHEMES.iter().find(|s| s.colors.as_slice() == c.colors.as_slice()) {
        Some(s) => s.name.to_string(),
        None => {
            if name == "custom" {
                name
            } else {
                "custom".into()
            }
        }
    };
    c.touch();
    true
}

// ----------------------------------------------------------------------
// Типы файлов
// ----------------------------------------------------------------------

/// Диалог «Типы файлов и подсветка».
pub fn syntax_dialog(app: &mut Program, cfg: &Shared) -> bool {
    let working = Rc::new(RefCell::new(cfg.borrow().filetypes.clone()));
    let syntax_on = cfg.borrow().options.syntax;
    let mut d = Dialog::centered(72, 21, "Типы файлов и подсветка синтаксиса");
    // Список языков в порядке LANGS; типы файлов ищутся по id.
    let items: Vec<String> = LANGS.iter().map(|l| l.name.to_string()).collect();
    let list_id = add_list(&mut d, Rect::new(2, 2, 25, 18), items);
    d.add(Label::new(Rect::new(2, 1, 20, 2), "~Я~зык", Some(list_id)));

    let pat_id = d.add(InputLine::new(Rect::new(28, 3, 70, 4), 300));
    d.add(Label::new(Rect::new(28, 2, 70, 3), "~Ф~айлы (расширения и имена через пробел):", Some(pat_id)));
    let ts_id = num_input(&mut d, 63, 5, "~Т~абуляция языка (0 — общая):", 28, 0, 32);
    let et_id =
        d.add(RadioButtons::radio_buttons(Rect::new(28, 7, 70, 10), &["как в параметрах редактора", "да, пробелы", "нет, символы табуляции"]));
    d.add(Label::new(Rect::new(28, 6, 70, 7), "~П~робелы вместо табуляции:", Some(et_id)));
    let mut on = CheckBoxes::check_boxes(Rect::new(28, 11, 70, 12), &["Подсветка синтаксиса включена"]);
    on.value = syntax_on as u32;
    let on_id = d.add(on);
    d.add(StaticText::new(
        Rect::new(28, 13, 70, 17),
        "Пример: «rs», «c h», «Makefile *.mk». Имя с заглавной буквы или с точкой сравнивается с именем файла целиком.\nТип текущего окна: :set ft=язык",
    ));
    d.add(Button::new(Rect::new(28, 18, 40, 20), "O~K~", cm::OK, bf::DEFAULT));
    d.add(Button::new(Rect::new(42, 18, 54, 20), "Отмена", cm::CANCEL, bf::NORMAL));
    d.add(Button::new(Rect::new(56, 18, 70, 20), "~С~бросить", CM_RESET, bf::NORMAL));
    d.select(list_id);

    fn load(d: &mut Dialog, w: &[crate::config::FileType], i: usize, pat_id: ViewId, ts_id: ViewId, et_id: ViewId) {
        let id = LANGS[i].id;
        if let Some(ft) = w.iter().find(|f| f.lang == id) {
            input_mut(d, pat_id).set_text(&ft.patterns);
            input_mut(d, ts_id).set_text(&ft.tabstop.to_string());
            cluster_mut(d, et_id).value = match ft.expandtab {
                None => 0,
                Some(true) => 1,
                Some(false) => 2,
            };
        }
    }
    fn store(d: &Dialog, w: &mut [crate::config::FileType], i: usize, pat_id: ViewId, ts_id: ViewId, et_id: ViewId) {
        let id = LANGS[i].id;
        if let Some(ft) = w.iter_mut().find(|f| f.lang == id) {
            ft.patterns = input(d, pat_id).text().trim().to_string();
            ft.tabstop = input(d, ts_id).text().trim().parse::<usize>().unwrap_or(0).min(32);
            ft.expandtab = match cluster(d, et_id).value {
                1 => Some(true),
                2 => Some(false),
                _ => None,
            };
        }
    }
    load(&mut d, &working.borrow(), 0, pat_id, ts_id, et_id);

    let mut h = HookDialog::new(d);
    let w1 = working.clone();
    h.on_command = Some(Box::new(move |d, c, _ctx| {
        if c != CM_RESET {
            return false;
        }
        let i = list(d, list_id).focused;
        let id = LANGS[i].id;
        if let Some(def) = default_filetypes().into_iter().find(|f| f.lang == id) {
            if let Some(ft) = w1.borrow_mut().iter_mut().find(|f| f.lang == id) {
                *ft = def;
            }
        }
        load(d, &w1.borrow(), i, pat_id, ts_id, et_id);
        true
    }));
    let w2 = working.clone();
    let mut last = 0usize;
    h.after = Some(Box::new(move |d, _ctx| {
        let i = list(d, list_id).focused.min(LANGS.len() - 1);
        if i != last {
            store(d, &mut w2.borrow_mut(), last, pat_id, ts_id, et_id);
            last = i;
            load(d, &w2.borrow(), i, pat_id, ts_id, et_id);
        }
    }));
    let (cmd, h) = app.exec_dialog(h);
    if cmd != cm::OK {
        return false;
    }
    let i = list(&h.dlg, list_id).focused.min(LANGS.len() - 1);
    store(&h.dlg, &mut working.borrow_mut(), i, pat_id, ts_id, et_id);
    let mut c = cfg.borrow_mut();
    c.filetypes = working.borrow().clone();
    c.options.syntax = cluster(&h.dlg, on_id).value & 1 != 0;
    c.touch();
    true
}

/// Выбор языка подсветки для текущего окна. `Some("")` — без подсветки.
pub fn filetype_dialog(app: &mut Program, current: Option<&str>) -> Option<String> {
    let mut d = Dialog::centered(40, 21, "Тип файла этого окна");
    let mut items = vec!["(обычный текст)".to_string()];
    items.extend(LANGS.iter().map(|l| format!("{}  [{}]", l.name, l.id)));
    let list_id = add_list(&mut d, Rect::new(2, 2, 37, 17), items);
    d.add(Label::new(Rect::new(2, 1, 30, 2), "~Я~зык подсветки", Some(list_id)));
    if let Some(cur) = current {
        if let Some(i) = LANGS.iter().position(|l| l.id == cur) {
            list_mut(&mut d, list_id).focus_item(i + 1);
        }
    }
    d.add(Button::new(Rect::new(6, 18, 18, 20), "O~K~", cm::OK, bf::DEFAULT));
    d.add(Button::new(Rect::new(20, 18, 32, 20), "Отмена", cm::CANCEL, bf::NORMAL));
    d.select(list_id);
    let (cmd, d) = app.exec_dialog(d);
    if cmd != cm::OK {
        return None;
    }
    let i = list(&d, list_id).focused;
    Some(if i == 0 { String::new() } else { LANGS[i - 1].id.to_string() })
}

// ----------------------------------------------------------------------
// Сопоставления клавиш
// ----------------------------------------------------------------------

fn mapping_line(m: &Mapping) -> String {
    format!("{:<10} {:<14} → {}", m.mode.title(), m.lhs, m.rhs)
}

/// Диалог «Сопоставления клавиш».
pub fn keymap_dialog(app: &mut Program, cfg: &Shared) -> bool {
    let leader = cfg.borrow().options.leader;
    let working = Rc::new(RefCell::new(cfg.borrow().mappings.clone()));
    let mut d = Dialog::centered(78, 22, "Сопоставления клавиш (:map)");
    let items: Vec<String> = working.borrow().iter().map(mapping_line).collect();
    let list_id = add_list(&mut d, Rect::new(2, 2, 46, 14), items);
    d.add(Label::new(Rect::new(2, 1, 30, 2), "~С~опоставления", Some(list_id)));

    let modes: Vec<&str> = MapMode::ALL.iter().map(|m| m.title()).collect();
    let mode_id = d.add(RadioButtons::radio_buttons(Rect::new(49, 2, 76, 5), &modes));
    d.add(Label::new(Rect::new(49, 1, 76, 2), "~Р~ежим", Some(mode_id)));
    let lhs_id = d.add(InputLine::new(Rect::new(49, 6, 76, 7), 60));
    d.add(Label::new(Rect::new(49, 5, 76, 6), "~К~лавиши:", Some(lhs_id)));
    let rhs_id = d.add(InputLine::new(Rect::new(49, 8, 76, 9), 200));
    d.add(Label::new(Rect::new(49, 7, 76, 8), "~Д~ействие:", Some(rhs_id)));
    d.add(Button::new(Rect::new(49, 10, 62, 12), "Доб~а~вить", CM_ADD, bf::NORMAL));
    d.add(Button::new(Rect::new(63, 10, 76, 12), "~И~зменить", CM_CHANGE, bf::NORMAL));
    d.add(Button::new(Rect::new(49, 12, 62, 14), "~У~далить", CM_DELETE, bf::NORMAL));
    let status_id = d.add(StaticText::new(
        Rect::new(2, 15, 76, 19),
        &format!(
            "Клавиши: <Esc> <CR> <Tab> <BS> <Space> <C-x> <F5> <Up> <Leader>=«{}»\nПримеры: Вставка  jk → <Esc>     Обычный  <C-s> → :w<CR>\nДействие выполняется как набранные клавиши (без рекурсии).",
            leader
        ),
    ));
    d.add(Button::new(Rect::new(24, 19, 36, 21), "O~K~", cm::OK, bf::DEFAULT));
    d.add(Button::new(Rect::new(38, 19, 50, 21), "Отмена", cm::CANCEL, bf::NORMAL));
    d.select(list_id);

    fn load(d: &mut Dialog, w: &[Mapping], i: usize, ids: (ViewId, ViewId, ViewId)) {
        if let Some(m) = w.get(i) {
            cluster_mut(d, ids.0).value = MapMode::ALL.iter().position(|x| *x == m.mode).unwrap_or(0) as u32;
            input_mut(d, ids.1).set_text(&m.lhs);
            input_mut(d, ids.2).set_text(&m.rhs);
        }
    }
    let ids = (mode_id, lhs_id, rhs_id);
    if !working.borrow().is_empty() {
        load(&mut d, &working.borrow(), 0, ids);
    }

    let mut h = HookDialog::new(d);
    let w1 = working.clone();
    h.on_command = Some(Box::new(move |d, c, _ctx| {
        if !matches!(c, CM_ADD | CM_CHANGE | CM_DELETE) {
            return false;
        }
        let sel = list(d, list_id).focused;
        let mode = MapMode::ALL[(cluster(d, mode_id).value as usize).min(2)];
        let lhs = input(d, lhs_id).text().trim().to_string();
        let rhs = input(d, rhs_id).text().to_string();
        let msg;
        if c == CM_DELETE {
            let mut w = w1.borrow_mut();
            if sel < w.len() {
                w.remove(sel);
                msg = "Удалено.".to_string();
            } else {
                msg = "Нечего удалять.".to_string();
            }
        } else {
            let check = parse_keys(&lhs, leader)
                .and_then(|l| if l.is_empty() { Err("пустые клавиши".into()) } else { Ok(l) })
                .and_then(|_| parse_keys(&rhs, leader));
            match check {
                Err(e) => {
                    if let Some(t) = d.win.group.find_mut::<StaticText>(status_id) {
                        t.text = format!("Ошибка: {}", e);
                    }
                    return true;
                }
                Ok(_) => {
                    let mut w = w1.borrow_mut();
                    let m = Mapping { mode, lhs, rhs };
                    if c == CM_CHANGE && sel < w.len() {
                        w[sel] = m;
                        msg = "Изменено.".into();
                    } else {
                        w.retain(|x| !(x.mode == m.mode && x.lhs == m.lhs));
                        w.push(m);
                        msg = "Добавлено.".into();
                    }
                }
            }
        }
        let items: Vec<String> = w1.borrow().iter().map(mapping_line).collect();
        let n = items.len();
        let lb = list_mut(d, list_id);
        lb.set_items(items);
        if n > 0 {
            lb.focus_item(if c == CM_ADD { n - 1 } else { sel.min(n - 1) });
        }
        if let Some(t) = d.win.group.find_mut::<StaticText>(status_id) {
            t.text = msg;
        }
        true
    }));
    let w2 = working.clone();
    let mut last = usize::MAX;
    h.after = Some(Box::new(move |d, _ctx| {
        let i = list(d, list_id).focused;
        if i != last && list(d, list_id).vs.is_focused() {
            last = i;
            load(d, &w2.borrow(), i, ids);
        }
    }));
    let (cmd, _h) = app.exec_dialog(h);
    if cmd != cm::OK {
        return false;
    }
    let mut c = cfg.borrow_mut();
    c.mappings = working.borrow().clone();
    c.touch();
    true
}

/// Окно «О программе».
pub fn about(app: &mut Program) {
    message_box_rect(
        app,
        Rect::new(0, 0, 54, 13),
        "\x03TVim 0.1\n\x03Текстовый редактор в духе vim\n\x03на библиотеке tvision-rs (Turbo Vision для Rust)\n\n\x03Режимы, операторы, регистры, макросы,\n\x03поиск и замена, подсветка синтаксиса.\n\x03:help — справка, F9 — параметры.",
        mf::INFORMATION | mf::OK_BUTTON,
        true,
    );
}

/// Настройки по умолчанию (для тестов диалогов).
pub fn default_shared() -> Shared {
    Config::default().shared()
}
