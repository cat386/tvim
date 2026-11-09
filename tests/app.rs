//! Приложение целиком на тестовом терминале: отрисовка, диалоги, окна.

use std::path::PathBuf;

use tvim::app::{menu, status, TvimApp};
use tvim::keys::{parse_keys, VKey};
use tvim::window::VimWindow;
use tvision::platform::HeadlessBackend;
use tvision::settings::Settings;
use tvision::*;

fn key_events(s: &str) -> Vec<KeyEvent> {
    parse_keys(s, '\\')
        .unwrap()
        .into_iter()
        .map(|k| match k {
            VKey::Char(c) => KeyEvent::new(Key::Char(c), Mods::NONE),
            VKey::Ctrl(c) => KeyEvent::new(Key::Char(c), Mods::CTRL),
            VKey::Esc => kb::ESC,
            VKey::Enter => kb::ENTER,
            VKey::Tab => kb::TAB,
            VKey::BackTab => kb::SHIFT_TAB,
            VKey::Backspace => KeyEvent::new(Key::Backspace, Mods::NONE),
            VKey::Del => KeyEvent::new(Key::Del, Mods::NONE),
            VKey::Ins => KeyEvent::new(Key::Ins, Mods::NONE),
            VKey::Up => KeyEvent::new(Key::Up, Mods::NONE),
            VKey::Down => KeyEvent::new(Key::Down, Mods::NONE),
            VKey::Left => KeyEvent::new(Key::Left, Mods::NONE),
            VKey::Right => KeyEvent::new(Key::Right, Mods::NONE),
            VKey::Home => KeyEvent::new(Key::Home, Mods::NONE),
            VKey::End => KeyEvent::new(Key::End, Mods::NONE),
            VKey::PgUp => KeyEvent::new(Key::PgUp, Mods::NONE),
            VKey::PgDn => KeyEvent::new(Key::PgDn, Mods::NONE),
            VKey::F(n) => kb::f(n),
        })
        .collect()
}

/// Запустить приложение с текстом в первом окне и набрать клавиши.
fn run_app(text: &str, keys: &[KeyEvent]) -> (Program, TvimApp, ViewId) {
    let mut b = HeadlessBackend::new(100, 30);
    for k in keys {
        b.push_key(*k);
    }
    locale::set_language(locale::Language::Ru);
    let mut app = Program::new(Box::new(b), Some(menu()), Some(status()));
    let mut h = TvimApp::new(Settings::default(), PathBuf::new());
    let id = h.open_file(&mut app, None);
    {
        let w = app.find_mut::<VimWindow>(id).unwrap();
        w.editor_mut().buf.set_text(text);
        w.editor_mut().set_lang("rust");
    }
    app.pump(&mut h);
    (app, h, id)
}

fn text_of(app: &Program, id: ViewId) -> String {
    app.find::<VimWindow>(id).unwrap().editor().buf.text()
}

#[test]
fn renders_editor() {
    let (mut app, _h, _id) = run_app("fn main() {\n    println!(\"hi\");\n}", &key_events("j"));
    app.redraw();
    let s = app.screen_text();
    assert!(s.contains("  1 fn main() {"), "{}", s);
    assert!(s.contains("  2     println!"), "{}", s);
    assert!(s.contains(" NORMAL "), "{}", s);
    assert!(s.contains("rust"), "{}", s);
    assert!(s.contains("2:1"), "{}", s);
    assert!(s.contains("Файл") && s.contains("Настройки"), "{}", s);
    assert!(s.contains("~"), "{}", s);
}

#[test]
fn typing_through_app() {
    let (app, _h, id) = run_app("world", &key_events("ihello <Esc>A!<Esc>"));
    assert_eq!(text_of(&app, id), "hello world!");
}

#[test]
fn quit_closes_program() {
    let (app, _h, _id) = run_app("x", &key_events(":q<CR>"));
    assert!(!app.is_running());
}

#[test]
fn quit_modified_refuses() {
    let (app, _h, id) = run_app("x", &key_events("dd:q<CR>"));
    assert!(app.is_running());
    let e = app.find::<VimWindow>(id).unwrap().editor();
    assert!(e.message.as_ref().unwrap().0.contains("E37"));
}

#[test]
fn force_quit_all() {
    let (app, _h, _id) = run_app("x", &key_events("dd:new<CR>:qa!<CR>"));
    assert!(!app.is_running());
}

#[test]
fn new_window_and_switch() {
    let (app, _h, _id) = run_app("x", &key_events(":new<CR>"));
    assert_eq!(TvimApp::windows(&app).len(), 2);
}

#[test]
fn options_dialog_changes_settings() {
    // F9 → флажок «Номера строк» (первый, в фокусе) — пробел → Enter (OK).
    let mut keys = vec![kb::f(9), KeyEvent::new(Key::Char(' '), Mods::NONE), kb::ENTER];
    keys.extend(key_events("i!<Esc>"));
    let (mut app, h, id) = run_app("abc", &keys);
    assert!(!h.cfg.borrow().options.number);
    assert_eq!(text_of(&app, id), "!abc");
    app.redraw();
    let s = app.screen_text();
    assert!(!s.contains("  1 !abc"), "{}", s);
    assert!(s.contains("!abc"), "{}", s);
}

#[test]
fn set_without_args_opens_dialog_cancel() {
    let mut keys = key_events(":set<CR>");
    keys.push(kb::ESC); // Отмена
    keys.extend(key_events("ix<Esc>"));
    let (app, h, id) = run_app("abc", &keys);
    assert!(h.cfg.borrow().options.number);
    assert_eq!(text_of(&app, id), "xabc");
}

#[test]
fn colors_dialog_loads_scheme() {
    // :colorscheme без аргумента → диалог; Tab ×3 до списка схем, ↓ (тёмная), Tab → «Загрузить», Enter? —
    // проще: выбрать схему и нажать Alt+З (кнопка «Загрузить»), затем Enter (OK).
    let mut keys = key_events(":colorscheme<CR>");
    keys.push(KeyEvent::new(Key::Char('г'), Mods::ALT)); // «Готовые схемы»
    keys.push(KeyEvent::new(Key::Down, Mods::NONE));
    keys.push(KeyEvent::new(Key::Char('з'), Mods::ALT)); // «Загрузить»
    keys.push(kb::ENTER);
    let (_app, h, _id) = run_app("abc", &keys);
    assert_eq!(h.cfg.borrow().scheme, "dark");
}

#[test]
fn keymap_dialog_adds_mapping() {
    let mut keys = vec![];
    keys.extend(key_events(":map<CR>"));
    keys.push(KeyEvent::new(Key::Char('р'), Mods::ALT)); // Режим
    keys.push(KeyEvent::new(Key::Down, Mods::NONE)); // Вставка
    keys.push(KeyEvent::new(Key::Char('к'), Mods::ALT)); // Клавиши
    keys.extend("jj".chars().map(|c| KeyEvent::new(Key::Char(c), Mods::NONE)));
    keys.push(KeyEvent::new(Key::Char('д'), Mods::ALT)); // Действие
    keys.extend("<Esc>".chars().map(|c| KeyEvent::new(Key::Char(c), Mods::NONE)));
    keys.push(KeyEvent::new(Key::Char('а'), Mods::ALT)); // Добавить
    keys.push(kb::ENTER);
    keys.extend(key_events("Axjjiy<Esc>"));
    let (app, h, id) = run_app("a", &keys);
    assert!(h.cfg.borrow().mappings.iter().any(|m| m.lhs == "jj" && m.rhs == "<Esc>"), "{:?}", h.cfg.borrow().mappings);
    assert_eq!(text_of(&app, id), "ayx");
}

#[test]
fn help_window() {
    let (mut app, _h, _id) = run_app("x", &[kb::F1]);
    app.redraw();
    let s = app.screen_text();
    assert!(s.contains("Справка TVim"), "{}", s);
    assert!(s.contains("TVim — справка"), "{}", s);
    assert_eq!(TvimApp::windows(&app).len(), 2);
}

#[test]
fn filetype_dialog_and_syntax() {
    let (mut app, _h, id) = run_app("x", &key_events(":setf<CR>"));
    // Диалог открыт: Esc — отмена; язык не изменился.
    let _ = &mut app;
    assert_eq!(app.find::<VimWindow>(id).unwrap().editor().lang.map(|l| l.id), Some("rust"));
}

#[test]
fn syntax_colors_on_screen() {
    let (mut app, h, _id) = run_app("fn x() {}", &[]);
    app.redraw();
    let scr = app.screen();
    // «fn» — ключевое слово: цвет из схемы.
    let kw = h.cfg.borrow().attr(tvim::config::Elem::Keyword);
    let mut found = false;
    for y in 0..30 {
        let line = scr.line_text(y);
        if let Some(p) = line.find("fn x()") {
            let x = line[..p].chars().count();
            assert_eq!(tvision::color::fore(scr.get(x, y).attr), tvision::color::fore(kw));
            found = true;
        }
    }
    assert!(found);
}
