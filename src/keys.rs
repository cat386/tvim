//! Клавиши редактора.
//!
//! События клавиатуры tvision ([`tvision::KeyEvent`]) переводятся в
//! упрощённое представление [`VKey`] — так их удобнее разбирать в стиле vim
//! (`d2w`, `"ayy`, `ci(`), записывать в макросы и повторять командой `.`.
//!
//! Здесь же — разбор и печать «нотации vim» для сопоставлений клавиш:
//! `<Esc>`, `<CR>`, `<C-x>`, `<Leader>`, `<F5>` и т.д.

use tvision::{Key, KeyEvent};

/// Клавиша в понятиях vim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VKey {
    /// Печатный символ.
    Char(char),
    /// Ctrl+буква (буква всегда строчная).
    Ctrl(char),
    Esc,
    Enter,
    Tab,
    BackTab,
    Backspace,
    Del,
    Ins,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PgUp,
    PgDn,
    /// Функциональная клавиша F1..F12.
    F(u8),
}

impl VKey {
    /// Перевести событие tvision. Alt+клавиши не переводятся — они
    /// принадлежат меню (Alt+F — «Файл» и т.п.).
    pub fn from_event(k: KeyEvent) -> Option<VKey> {
        if k.mods.alt {
            return None;
        }
        Some(match k.key {
            Key::Char(c) if k.mods.ctrl => match c.to_ascii_lowercase() {
                // Классические синонимы терминала.
                '[' => VKey::Esc,
                'm' => VKey::Enter,
                'i' => VKey::Tab,
                c => VKey::Ctrl(c),
            },
            Key::Char(c) => VKey::Char(c),
            Key::Enter => VKey::Enter,
            Key::Esc => VKey::Esc,
            Key::Tab if k.mods.shift => VKey::BackTab,
            Key::Tab => VKey::Tab,
            Key::BackTab => VKey::BackTab,
            Key::Backspace => VKey::Backspace,
            Key::Del => VKey::Del,
            Key::Ins => VKey::Ins,
            Key::Up => VKey::Up,
            Key::Down => VKey::Down,
            Key::Left => VKey::Left,
            Key::Right => VKey::Right,
            Key::Home => VKey::Home,
            Key::End => VKey::End,
            Key::PgUp => VKey::PgUp,
            Key::PgDn => VKey::PgDn,
            Key::F(n) => VKey::F(n),
        })
    }

    /// Символ, если это обычная печатная клавиша.
    pub fn ch(self) -> Option<char> {
        match self {
            VKey::Char(c) => Some(c),
            _ => None,
        }
    }
}

/// Названия специальных клавиш в нотации vim (без учёта регистра).
const NAMES: &[(&str, VKey)] = &[
    ("esc", VKey::Esc),
    ("cr", VKey::Enter),
    ("enter", VKey::Enter),
    ("return", VKey::Enter),
    ("tab", VKey::Tab),
    ("s-tab", VKey::BackTab),
    ("bs", VKey::Backspace),
    ("backspace", VKey::Backspace),
    ("del", VKey::Del),
    ("delete", VKey::Del),
    ("ins", VKey::Ins),
    ("insert", VKey::Ins),
    ("up", VKey::Up),
    ("down", VKey::Down),
    ("left", VKey::Left),
    ("right", VKey::Right),
    ("home", VKey::Home),
    ("end", VKey::End),
    ("pageup", VKey::PgUp),
    ("pagedown", VKey::PgDn),
    ("space", VKey::Char(' ')),
    ("lt", VKey::Char('<')),
    ("bar", VKey::Char('|')),
    ("bslash", VKey::Char('\\')),
];

/// Разобрать запись вида `jk`, `<Esc>`, `<C-s>`, `<Leader>w` в список клавиш.
/// `leader` — чем заменить `<Leader>`.
pub fn parse_keys(s: &str, leader: char) -> Result<Vec<VKey>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '<' {
            if let Some(len) = chars[i + 1..].iter().position(|&x| x == '>') {
                let name: String = chars[i + 1..i + 1 + len].iter().collect();
                let low = name.to_lowercase();
                let key = if low == "leader" {
                    Some(VKey::Char(leader))
                } else if let Some((_, k)) = NAMES.iter().find(|(n, _)| *n == low) {
                    Some(*k)
                } else if let Some(rest) = low.strip_prefix("c-") {
                    let cs: Vec<char> = rest.chars().collect();
                    if cs.len() == 1 {
                        Some(match cs[0] {
                            '[' => VKey::Esc,
                            'm' => VKey::Enter,
                            'i' => VKey::Tab,
                            c => VKey::Ctrl(c),
                        })
                    } else {
                        None
                    }
                } else if let Some(n) = low.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                    if (1..=12).contains(&n) {
                        Some(VKey::F(n))
                    } else {
                        None
                    }
                } else {
                    None
                };
                match key {
                    Some(k) => {
                        out.push(k);
                        i += len + 2;
                        continue;
                    }
                    None if len > 0 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
                        return Err(format!("неизвестная клавиша <{}>", name))
                    }
                    None => {}
                }
            }
        }
        out.push(VKey::Char(c));
        i += 1;
    }
    Ok(out)
}

/// Записать клавиши в нотации vim (обратное к [`parse_keys`]).
pub fn format_keys(keys: &[VKey]) -> String {
    let mut s = String::new();
    for k in keys {
        match k {
            VKey::Char('<') => s.push_str("<lt>"),
            VKey::Char(' ') => s.push_str("<Space>"),
            VKey::Char(c) => s.push(*c),
            VKey::Ctrl(c) => s.push_str(&format!("<C-{}>", c)),
            VKey::Esc => s.push_str("<Esc>"),
            VKey::Enter => s.push_str("<CR>"),
            VKey::Tab => s.push_str("<Tab>"),
            VKey::BackTab => s.push_str("<S-Tab>"),
            VKey::Backspace => s.push_str("<BS>"),
            VKey::Del => s.push_str("<Del>"),
            VKey::Ins => s.push_str("<Insert>"),
            VKey::Up => s.push_str("<Up>"),
            VKey::Down => s.push_str("<Down>"),
            VKey::Left => s.push_str("<Left>"),
            VKey::Right => s.push_str("<Right>"),
            VKey::Home => s.push_str("<Home>"),
            VKey::End => s.push_str("<End>"),
            VKey::PgUp => s.push_str("<PageUp>"),
            VKey::PgDn => s.push_str("<PageDown>"),
            VKey::F(n) => s.push_str(&format!("<F{}>", n)),
        }
    }
    s
}

/// Перевести строку в клавиши «как есть» (для `:normal` и тестов).
pub fn text_keys(s: &str) -> Vec<VKey> {
    s.chars()
        .map(|c| match c {
            '\n' => VKey::Enter,
            '\x1b' => VKey::Esc,
            '\t' => VKey::Tab,
            c => VKey::Char(c),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notation_roundtrip() {
        let k = parse_keys("<Leader>w<C-s>jk<Esc><lt>x<F5>", '\\').unwrap();
        assert_eq!(
            k,
            vec![
                VKey::Char('\\'),
                VKey::Char('w'),
                VKey::Ctrl('s'),
                VKey::Char('j'),
                VKey::Char('k'),
                VKey::Esc,
                VKey::Char('<'),
                VKey::Char('x'),
                VKey::F(5)
            ]
        );
        assert_eq!(format_keys(&k), "\\w<C-s>jk<Esc><lt>x<F5>");
        assert!(parse_keys("<Foo>", ' ').is_err());
        // Одинокая «<» — просто символ.
        assert_eq!(parse_keys("a<b", ' ').unwrap(), vec![VKey::Char('a'), VKey::Char('<'), VKey::Char('b')]);
    }
}
