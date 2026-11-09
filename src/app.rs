//! Приложение TVim: меню, строка состояния, окна, файл настроек.

use std::path::{Path, PathBuf};

use tvision::settings::{settings_dialog, Settings};
use tvision::*;

use crate::config::{Config, Shared};
use crate::dialogs;
use crate::editor::{cmd, CmdKind, VimEditor};
use crate::window::VimWindow;

/// Меню.
pub fn menu() -> Menu {
    Menu::new()
        .submenu(
            "~Ф~айл",
            Menu::new()
                .item("~Н~овый", cm::NEW)
                .item_key("~О~ткрыть...", cm::OPEN, kb::F3, "F3")
                .item_key("~З~аписать", cm::SAVE, kb::F2, "F2")
                .item("Записать ~к~ак...", cm::SAVE_AS)
                .item("Записать ~в~се", cmd::WRITE_ALL)
                .separator()
                .item("~С~менить каталог...", cm::CH_DIR)
                .separator()
                .item_key("~В~ыход", cm::QUIT, kb::ALT_X, "Alt-X"),
        )
        .submenu(
            "~П~равка",
            Menu::new()
                .item("~О~тменить           u", cm::UNDO)
                .item("~П~овторить     Ctrl-R", cm::REDO)
                .separator()
                .item_key("~В~ырезать", cm::CUT, KeyEvent::new(Key::Del, Mods::SHIFT), "Shift-Del")
                .item_key("~К~опировать", cm::COPY, KeyEvent::new(Key::Ins, Mods::CTRL), "Ctrl-Ins")
                .item_key("Вст~а~вить", cm::PASTE, KeyEvent::new(Key::Ins, Mods::SHIFT), "Shift-Ins")
                .separator()
                .item("Выделить ~в~сё     ggVG", cm::SELECT_ALL),
        )
        .submenu(
            "По~и~ск",
            Menu::new()
                .item("~Н~айти...           /", cm::FIND)
                .item("~З~аменить...    :%s//", cm::REPLACE)
                .item("Искать ~д~альше      n", cm::SEARCH_AGAIN)
                .separator()
                .item("~П~ерейти к строке...  :", cmd::GOTO_LINE),
        )
        .submenu(
            "~Н~астройки",
            Menu::new()
                .item_key("~П~араметры редактора...", cmd::OPTIONS, kb::f(9), "F9")
                .item("~Ц~ветовая схема...", cmd::COLORS)
                .item("~Т~ипы файлов и подсветка...", cmd::SYNTAX)
                .item("Тип файла ~э~того окна...", cmd::SET_FILETYPE)
                .item("~С~опоставления клавиш...", cmd::KEYMAP)
                .separator()
                .item("~О~бщие настройки (тема, язык)...", cmd::APP_SETTINGS),
        )
        .submenu(
            "~О~кно",
            Menu::new()
                .item("~М~озаика", cm::TILE)
                .item("~К~аскад", cm::CASCADE)
                .separator()
                .item_key("~Р~аспахнуть", cm::ZOOM, kb::F5, "F5")
                .item_key("~С~ледующее", cm::NEXT, kb::F6, "F6")
                .item_key("~П~редыдущее", cm::PREV, kb::SHIFT_F6, "Shift-F6")
                .item_key("~З~акрыть", cm::CLOSE, kb::ALT_F3, "Alt-F3"),
        )
        .submenu("~С~правка", Menu::new().item_key("~С~правка", cmd::HELP, kb::F1, "F1").separator().item("~О~ программе...", CM_ABOUT))
}

const CM_ABOUT: u16 = 320;

/// Строка состояния.
pub fn status() -> Vec<StatusDef> {
    vec![StatusDef::all(vec![
        StatusItem::new("~F1~ Справка", kb::F1, cmd::HELP),
        StatusItem::new("~F2~ Записать", kb::F2, cm::SAVE),
        StatusItem::new("~F3~ Открыть", kb::F3, cm::OPEN),
        StatusItem::new("~F9~ Параметры", kb::f(9), cmd::OPTIONS),
        StatusItem::new("~F10~ Меню", kb::F10, cm::MENU),
        StatusItem::new("~Alt-X~ Выход", kb::ALT_X, cm::QUIT),
        StatusItem::new("", kb::F5, cm::ZOOM),
        StatusItem::new("", kb::F6, cm::NEXT),
        StatusItem::new("", kb::SHIFT_F6, cm::PREV),
    ])]
}

/// Ключ настроек редактора в файле настроек tvision.
pub const SETTINGS_KEY: &str = "tvim";

pub struct TvimApp {
    pub cfg: Shared,
    pub settings: Settings,
    pub settings_path: PathBuf,
}

impl TvimApp {
    pub fn new(settings: Settings, settings_path: PathBuf) -> Self {
        let cfg = settings.get(SETTINGS_KEY).map(Config::from_json).unwrap_or_default();
        TvimApp { cfg: cfg.shared(), settings, settings_path }
    }

    /// Записать файл настроек.
    pub fn save_config(&mut self) -> Result<(), String> {
        let j = self.cfg.borrow().to_json();
        self.settings.set(SETTINGS_KEY, j);
        if self.settings_path.as_os_str().is_empty() {
            return Ok(());
        }
        self.settings.save(&self.settings_path)
    }

    /// Окна редактора на рабочем столе (сверху вниз).
    pub fn windows(app: &Program) -> Vec<ViewId> {
        app.desktop().group.children.iter().filter(|c| c.as_any().is::<VimWindow>()).map(|c| c.vs().id).collect()
    }

    /// Редактор активного окна.
    pub fn current_editor(app: &mut Program) -> Option<&mut VimEditor> {
        let desk = &app.desktop().group;
        let cur = desk.current.and_then(|i| desk.children.get(i)).map(|c| c.vs().id)?;
        let w = app.find_mut::<VimWindow>(cur)?;
        Some(w.editor_mut())
    }

    fn free_number(app: &Program) -> i16 {
        let used: Vec<i16> = Self::windows(app).iter().filter_map(|&id| app.find::<VimWindow>(id).map(|w| w.win.number)).collect();
        (1..10).find(|n| !used.contains(n)).unwrap_or(0)
    }

    /// Открыть файл (или новый буфер) в окне. Уже открытый файл
    /// просто выбирается.
    pub fn open_file(&mut self, app: &mut Program, path: Option<&Path>) -> ViewId {
        if let Some(p) = path {
            let abs = crate::editor::absolute(p);
            for id in Self::windows(app) {
                if app.find::<VimWindow>(id).map(|w| w.editor().file == abs).unwrap_or(false) {
                    app.focus(id);
                    return id;
                }
            }
            // Единственное пустое безымянное окно занимаем под файл.
            let ws = Self::windows(app);
            if ws.len() == 1 {
                if let Some(w) = app.find_mut::<VimWindow>(ws[0]) {
                    let e = w.editor_mut();
                    if e.file.as_os_str().is_empty() && !e.modified() && e.buf.line_count() == 1 && e.buf.line_len(0) == 0 {
                        if let Err(err) = e.open(p) {
                            e.error(&err);
                        }
                        return ws[0];
                    }
                }
            }
        }
        let r = app.desktop().group.vs.extent();
        let n = Self::free_number(app);
        let w = VimWindow::new(r, self.cfg.clone(), path, n);
        app.insert_window(w)
    }

    /// Открыть окно справки (на теме `topic`).
    pub fn open_help(&mut self, app: &mut Program, topic: &str) {
        // Уже открытое окно справки — переиспользуем.
        let mut id = None;
        for w in Self::windows(app) {
            if app.find::<VimWindow>(w).map(|x| x.editor().readonly && x.editor().file.as_os_str() == "help").unwrap_or(false) {
                id = Some(w);
            }
        }
        let id = match id {
            Some(i) => {
                app.focus(i);
                i
            }
            None => {
                let r = app.desktop().group.vs.extent();
                let mut w = VimWindow::new(r, self.cfg.clone(), None, 0);
                {
                    let e = w.editor_mut();
                    e.buf.set_text(crate::help::HELP.trim_end());
                    e.file = PathBuf::from("help");
                    e.readonly = true;
                    e.display_name = Some("Справка TVim".into());
                    e.set_lang("markdown");
                    e.message = Some(("Справка. :q — закрыть, /слово — поиск".into(), false));
                }
                app.insert_window(w)
            }
        };
        if !topic.trim().is_empty() {
            if let Some(w) = app.find_mut::<VimWindow>(id) {
                let e = w.editor_mut();
                let t = topic.trim().to_lowercase();
                let found = (0..e.buf.line_count()).find(|&l| {
                    let s = e.buf.line_string(l).to_lowercase();
                    s.starts_with('#') && s.contains(&t)
                });
                let found = found.or_else(|| (0..e.buf.line_count()).find(|&l| e.buf.line_string(l).to_lowercase().contains(&t)));
                match found {
                    Some(l) => {
                        e.cur = crate::buffer::Pos::new(l, 0);
                        e.delta.y = l as i32;
                    }
                    None => e.error(&format!("E149: Нет справки по теме «{}»", topic)),
                }
            }
        }
    }

    /// Применить настройки во всех окнах (после диалогов).
    fn after_config_change(&mut self, app: &mut Program) {
        if let Err(e) = self.save_config() {
            message_box(app, &e, mf::ERROR | mf::OK_BUTTON);
        }
        for id in Self::windows(app) {
            if let Some(w) = app.find_mut::<VimWindow>(id) {
                let e = w.editor_mut();
                e.last_sync = None;
                e.track_cursor();
            }
        }
    }

    fn file_open_dialog(&mut self, app: &mut Program) {
        let d = FileDialog::new("*", "Открыть файл", "~И~мя", fd::OPEN_BUTTON, 100);
        let (c, d) = app.exec_dialog(d);
        if c != cm::CANCEL {
            let name = d.file_name();
            self.open_file(app, Some(&name));
        }
    }
}

impl AppHandler for TvimApp {
    fn handle_event(&mut self, app: &mut Program, ev: &mut Event) {
        let Some(c) = ev.command() else { return };
        let info = match ev {
            Event::Command(m) => m.info.clone(),
            _ => Info::None,
        };
        match c {
            cm::NEW => {
                self.open_file(app, None);
            }
            cm::OPEN => self.file_open_dialog(app),
            cmd::EDIT_FILE => {
                let p = match info {
                    Info::Str(s) if !s.is_empty() => Some(PathBuf::from(s)),
                    _ => None,
                };
                self.open_file(app, p.as_deref());
            }
            cm::CH_DIR => {
                app.exec_dialog(ChDirDialog::new(cd::NORMAL, 0));
            }
            cmd::OPTIONS => {
                if dialogs::options_dialog(app, &self.cfg) {
                    self.after_config_change(app);
                }
            }
            cmd::COLORS => {
                if dialogs::colors_dialog(app, &self.cfg) {
                    self.after_config_change(app);
                }
            }
            cmd::SYNTAX => {
                if dialogs::syntax_dialog(app, &self.cfg) {
                    // Новые сопоставления типов — заново определить язык окон.
                    for id in Self::windows(app) {
                        if let Some(w) = app.find_mut::<VimWindow>(id) {
                            let e = w.editor_mut();
                            if !e.readonly {
                                e.detect_lang();
                            }
                        }
                    }
                    self.after_config_change(app);
                }
            }
            cmd::SET_FILETYPE => {
                let cur = Self::current_editor(app).and_then(|e| e.lang.map(|l| l.id));
                if let Some(id) = dialogs::filetype_dialog(app, cur) {
                    if let Some(e) = Self::current_editor(app) {
                        e.set_lang(&id);
                    }
                }
            }
            cmd::KEYMAP => {
                if dialogs::keymap_dialog(app, &self.cfg) {
                    self.after_config_change(app);
                }
            }
            cmd::APP_SETTINGS => {
                if settings_dialog(app, &mut self.settings) {
                    let _ = self.save_config();
                }
            }
            cmd::SAVE_CONFIG => {
                let _ = self.save_config();
            }
            cmd::HELP => {
                let topic = match info {
                    Info::Str(s) => s,
                    _ => String::new(),
                };
                self.open_help(app, &topic);
            }
            cmd::GOTO_LINE => {
                if let Some(e) = Self::current_editor(app) {
                    e.open_cmdline(CmdKind::Ex, "");
                }
            }
            cmd::QUIT_IF_EMPTY => {
                if Self::windows(app).is_empty() {
                    app.quit();
                }
            }
            cmd::QUIT_FORCE => {
                for id in Self::windows(app) {
                    if let Some(w) = app.find_mut::<VimWindow>(id) {
                        w.editor_mut().force_close = true;
                    }
                }
                app.quit();
            }
            cmd::QUIT_ALL => app.put_event(Event::command_event(cm::QUIT)),
            cmd::WRITE_ALL | cmd::WRITE_QUIT_ALL => {
                let mut errors = Vec::new();
                for id in Self::windows(app) {
                    if let Some(w) = app.find_mut::<VimWindow>(id) {
                        let e = w.editor_mut();
                        if e.modified() && !e.readonly {
                            if e.file.as_os_str().is_empty() {
                                errors.push("E141: У безымянного буфера нет имени файла".to_string());
                                continue;
                            }
                            match e.write_file(None) {
                                Ok(m) => e.info(&m),
                                Err(er) => errors.push(er),
                            }
                        }
                    }
                }
                if !errors.is_empty() {
                    message_box(app, &errors.join("\n"), mf::ERROR | mf::OK_BUTTON);
                } else if c == cmd::WRITE_QUIT_ALL {
                    app.quit();
                }
            }
            CM_ABOUT => dialogs::about(app),
            _ => return,
        }
        ev.clear();
    }
}

/// Запуск: настройки, файлы из командной строки, цикл событий.
pub fn run(files: &[PathBuf]) -> std::io::Result<()> {
    let path = Settings::default_path();
    let settings = Settings::load_or_default(&path);
    settings.apply_language();
    // Интерфейс редактора русский; стандартные диалоги — тоже.
    if settings.language == tvision::settings::LanguageChoice::System {
        locale::set_language(locale::Language::Ru);
    }
    let mut app = Program::with_terminal(Some(menu()), Some(status()))?;
    let theme_err = settings.apply(&mut app).err();
    if settings.language == tvision::settings::LanguageChoice::System {
        locale::set_language(locale::Language::Ru);
    }
    let mut h = TvimApp::new(settings, path);
    if files.is_empty() {
        h.open_file(&mut app, None);
    } else {
        for f in files {
            h.open_file(&mut app, Some(f));
        }
        if files.len() > 1 {
            app.put_event(Event::command_event(cm::TILE));
        }
    }
    if let Some(e) = theme_err {
        message_box(&mut app, &e, mf::WARNING | mf::OK_BUTTON);
    }
    app.run(&mut h);
    let _ = h.save_config();
    Ok(())
}
