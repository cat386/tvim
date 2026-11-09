//! Окно редактора: рамка с заголовком (имя файла), вертикальная полоса
//! прокрутки и [`VimEditor`] во всю внутреннюю область.

use std::path::Path;

use tvision::*;

use crate::config::Shared;
use crate::editor::VimEditor;

pub struct VimWindow {
    pub win: Window,
    pub editor_id: ViewId,
}

impl VimWindow {
    /// Окно с файлом (`None` — новый безымянный буфер).
    pub fn new(bounds: Rect, cfg: Shared, file: Option<&Path>, number: i16) -> Self {
        let mut win = Window::new(bounds, "", number);
        win.group.vs.options |= of::TILEABLE;
        let vsb = win.standard_scroll_bar(true);
        let mut ed = VimEditor::new(win.interior(), cfg, Some(vsb));
        if let Some(f) = file {
            if let Err(e) = ed.open(f) {
                ed.error(&e);
            }
        }
        let title = ed.title();
        let editor_id = win.add(ed);
        win.title = title;
        VimWindow { win, editor_id }
    }

    pub fn editor(&self) -> &VimEditor {
        self.win.group.find::<VimEditor>(self.editor_id).expect("редактор")
    }

    pub fn editor_mut(&mut self) -> &mut VimEditor {
        let id = self.editor_id;
        self.win.group.find_mut::<VimEditor>(id).expect("редактор")
    }

    fn update_title(&mut self) {
        let e = self.editor();
        let mut t = e.title();
        if e.modified() {
            t.push_str(" [+]");
        }
        if self.win.title != t {
            self.win.title = t;
        }
    }
}

impl View for VimWindow {
    tvision::impl_group_basics!(win.group);

    fn as_window(&self) -> Option<&Window> {
        Some(&self.win)
    }

    fn as_window_mut(&mut self) -> Option<&mut Window> {
        Some(&mut self.win)
    }

    fn draw(&self, dc: &mut DrawCtx) {
        self.win.draw_window(dc);
    }

    fn palette(&self) -> &'static [u8] {
        self.win.palette
    }

    fn set_state(&mut self, flag: u16, enable: bool) {
        self.win.set_window_state(flag, enable);
    }

    fn handle_event(&mut self, ev: &mut Event, ctx: &mut Ctx) {
        // Клавиши (и Tab тоже) сначала получает редактор — окно видит
        // только то, что он не обработал.
        self.win.handle_window_event(ev, ctx);
        self.update_title();
    }

    fn valid(&mut self, command: u16, ctx: &mut Ctx) -> bool {
        self.win.group.valid(command, ctx)
    }

    fn size_limits(&self, owner_size: Point) -> (Point, Point) {
        (Point::new(24, 6), owner_size)
    }
}
