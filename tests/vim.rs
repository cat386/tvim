//! Сценарии vim: редактор получает клавиши в нотации vim и проверяется
//! получившийся текст и положение курсора.

use tvim::buffer::Pos;
use tvim::config::Config;
use tvim::editor::{Mode, VKind, VimEditor};
use tvim::keys::parse_keys;
use tvision::{Ctx, Rect};

fn ed(text: &str) -> (VimEditor, Ctx) {
    let mut cfg = Config::default();
    cfg.options.clipboard = false;
    cfg.options.smartindent = false;
    cfg.options.expandtab = true;
    cfg.options.shiftwidth = 4;
    cfg.options.tabstop = 4;
    cfg.mappings.clear();
    let mut e = VimEditor::new(Rect::new(0, 0, 80, 24), cfg.shared(), None);
    e.buf.set_text(text);
    (e, Ctx::new())
}

fn run(e: &mut VimEditor, ctx: &mut Ctx, keys: &str) {
    let k = parse_keys(keys, '\\').unwrap();
    e.keys(&k, ctx);
}

/// Текст после клавиш.
fn t(text: &str, keys: &str) -> String {
    let (mut e, mut c) = ed(text);
    run(&mut e, &mut c, keys);
    e.buf.text()
}

#[test]
fn motions() {
    let (mut e, mut c) = ed("one two  three\nfour\n\nfive six");
    run(&mut e, &mut c, "w");
    assert_eq!(e.cur, Pos::new(0, 4));
    run(&mut e, &mut c, "w");
    assert_eq!(e.cur, Pos::new(0, 9));
    run(&mut e, &mut c, "w");
    assert_eq!(e.cur, Pos::new(1, 0));
    run(&mut e, &mut c, "w");
    assert_eq!(e.cur, Pos::new(2, 0)); // пустая строка — слово
    run(&mut e, &mut c, "b");
    assert_eq!(e.cur, Pos::new(1, 0));
    run(&mut e, &mut c, "gg$");
    assert_eq!(e.cur, Pos::new(0, 13));
    run(&mut e, &mut c, "0e");
    assert_eq!(e.cur, Pos::new(0, 2));
    run(&mut e, &mut c, "G");
    assert_eq!(e.cur, Pos::new(3, 0));
    run(&mut e, &mut c, "2G");
    assert_eq!(e.cur, Pos::new(1, 0));
    run(&mut e, &mut c, "ggfr");
    assert_eq!(e.cur, Pos::new(0, 11));
    run(&mut e, &mut c, "0tt");
    assert_eq!(e.cur, Pos::new(0, 3));
    run(&mut e, &mut c, ";");
    assert_eq!(e.cur, Pos::new(0, 8));
    run(&mut e, &mut c, "}");
    assert_eq!(e.cur, Pos::new(2, 0));
    // j/k помнят столбец.
    let (mut e, mut c) = ed("abcdef\nab\nabcdef");
    run(&mut e, &mut c, "$jj");
    assert_eq!(e.cur, Pos::new(2, 5));
    run(&mut e, &mut c, "0llllkj");
    assert_eq!(e.cur, Pos::new(2, 4));
}

#[test]
fn delete_change_yank() {
    assert_eq!(t("hello world", "dw"), "world");
    assert_eq!(t("hello world", "wD"), "hello ");
    assert_eq!(t("hello world", "x"), "ello world");
    assert_eq!(t("hello world", "3x"), "lo world");
    assert_eq!(t("hello world", "cwbye<Esc>"), "bye world");
    assert_eq!(t("hello world", "ceHI<Esc>"), "HI world");
    assert_eq!(t("a\nb\nc\nd", "jdd"), "a\nc\nd");
    assert_eq!(t("a\nb\nc\nd", "2dd"), "c\nd");
    assert_eq!(t("a\nb\nc\nd", "Gdd"), "a\nb\nc");
    assert_eq!(t("a\nb\nc\nd", "dj"), "c\nd");
    assert_eq!(t("a\nb\nc\nd", "dG"), "");
    assert_eq!(t("one\ntwo", "yyp"), "one\none\ntwo");
    assert_eq!(t("one\ntwo", "yyjP"), "one\none\ntwo");
    assert_eq!(t("abc", "ylp"), "aabc");
    assert_eq!(t("abc def", "yiwwviwp"), "abc abc");
    assert_eq!(t("hello world", "wcc new<Esc>"), " new");
    assert_eq!(t("  hello", "ccx<Esc>"), "  x");
    assert_eq!(t("hello", "d$"), "");
    assert_eq!(t("word1 word2\nnext", "wdw"), "word1 \nnext");
    assert_eq!(t("foo(bar, baz)", "fbdt)"), "foo()");
    assert_eq!(t("foo(bar, baz)", "fbdf,"), "foo( baz)");
    assert_eq!(t("a b", "d0"), "a b");
    assert_eq!(t("a b", "$d0"), "b");
    assert_eq!(t("abc", "lX"), "bc");
    assert_eq!(t("abcd", "lsX<Esc>"), "aXcd");
    assert_eq!(t("abcd\nef", "Snew<Esc>"), "new\nef");
    assert_eq!(t("abc def", "wC!<Esc>"), "abc !");
    assert_eq!(t("p1\n\np2a\np2b\n\np3", "jjd}"), "p1\n\n\np3");
}

#[test]
fn text_objects() {
    assert_eq!(t("call(a, b) x", "fbdi("), "call() x");
    assert_eq!(t("call(a, b) x", "fbda("), "call x");
    assert_eq!(t("say \"hi there\" ok", "fhci\"bye<Esc>"), "say \"bye\" ok");
    assert_eq!(t("say \"hi there\" ok", "da\""), "say ok");
    assert_eq!(t("one two three", "wdiw"), "one  three");
    assert_eq!(t("one two three", "wdaw"), "one three");
    assert_eq!(t("fn x() {\n    a;\n    b;\n}", "jdi{"), "fn x() {\n}");
    assert_eq!(t("p1\np1\n\np2", "dap"), "p2");
    assert_eq!(t("<a><b>x</b></a>", "fxdit"), "<a><b></b></a>");
    assert_eq!(t("<a><b>x</b></a>", "fxdat"), "<a></a>");
    assert_eq!(t("a[1, [2]]", "f2di["), "a[1, []]");
    assert_eq!(t("x = {k: v}", "fkyi{$p"), "x = {k: v}k: v");
}

#[test]
fn insert_modes() {
    assert_eq!(t("world", "ihello <Esc>"), "hello world");
    assert_eq!(t("abc", "aX<Esc>"), "aXbc");
    assert_eq!(t("abc", "AX<Esc>"), "abcX");
    assert_eq!(t("  abc", "$IX<Esc>"), "  Xabc");
    assert_eq!(t("a\nb", "oX<Esc>"), "a\nX\nb");
    assert_eq!(t("a\nb", "jOX<Esc>"), "a\nX\nb");
    assert_eq!(t("    a", "oX<Esc>"), "    a\n    X");
    assert_eq!(t("", "3ia<Esc>"), "aaa");
    assert_eq!(t("abc", "RXY<Esc>"), "XYc");
    assert_eq!(t("abc", "RXYZW<BS><BS><Esc>"), "XYc");
    assert_eq!(t("", "ione two<C-w>x<Esc>"), "one x");
    assert_eq!(t("", "i<Tab>x<Esc>"), "    x");
    assert_eq!(t("", "i<Tab><BS>x<Esc>"), "x");
    assert_eq!(t("ab", "A<CR>c<Esc>"), "ab\nc");
    assert_eq!(t("a\nb", "jA<BS><BS>x<Esc>"), "ax");
    assert_eq!(t("x", "A<C-t><Esc>"), "    x");
    // Ctrl-O: одна команда обычного режима.
    assert_eq!(t("abc", "A1<C-o>0X<Esc>"), "Xabc1");
    // Дополнение слова.
    assert_eq!(t("function\n", "jifu<C-n><Esc>"), "function\nfunction");
    // Ctrl-R регистр.
    assert_eq!(t("abc", "yiwA <C-r>\"<Esc>"), "abc abc");
}

#[test]
fn smartindent() {
    let (mut e, mut c) = ed("fn x() {");
    e.cfg.borrow_mut().options.smartindent = true;
    run(&mut e, &mut c, "A<CR>a;<CR>}<Esc>");
    assert_eq!(e.buf.text(), "fn x() {\n    a;\n}");
    let (mut e, mut c) = ed("f(");
    e.cfg.borrow_mut().options.smartindent = true;
    run(&mut e, &mut c, "A<CR>x<Esc>");
    assert_eq!(e.buf.text(), "f(\n    x");
}

#[test]
fn undo_redo_dot() {
    let (mut e, mut c) = ed("a b c d");
    run(&mut e, &mut c, "dw..");
    assert_eq!(e.buf.text(), "d");
    run(&mut e, &mut c, "u");
    assert_eq!(e.buf.text(), "c d");
    run(&mut e, &mut c, "uu");
    assert_eq!(e.buf.text(), "a b c d");
    run(&mut e, &mut c, "<C-r>");
    assert_eq!(e.buf.text(), "b c d");
    assert_eq!(t("x\nx\nx", "Ay<Esc>j.j."), "xy\nxy\nxy");
    assert_eq!(t("a a a a", "ciwb<Esc>w.w."), "b b b a");
    assert_eq!(t("1 2 3 4 5 6", "dw2."), "4 5 6");
    // Вся вставка отменяется одной командой.
    assert_eq!(t("", "ione<CR>two<Esc>u"), "");
    assert!(!{
        let (mut e, mut c) = ed("abc");
        run(&mut e, &mut c, "xu");
        e.modified()
    });
}

#[test]
fn visual_mode() {
    assert_eq!(t("hello world", "vlld"), "lo world");
    assert_eq!(t("hello world", "wvey0P"), "worldhello world");
    assert_eq!(t("a\nb\nc", "Vjd"), "c");
    assert_eq!(t("a\nb\nc", "Vj>"), "    a\n    b\nc");
    assert_eq!(t("abc\ndef", "vjU"), "ABC\nDef");
    assert_eq!(t("abc", "v$~"), "ABC");
    assert_eq!(t("a\nb\nc", "VjJ"), "a b\nc");
    assert_eq!(t("abc\ndef\nghi", "l<C-v>jjd"), "ac\ndf\ngi");
    assert_eq!(t("abc\ndef\nghi", "<C-v>jjI# <Esc>"), "# abc\n# def\n# ghi");
    assert_eq!(t("ab\ncd", "<C-v>j$A;<Esc>"), "ab;\ncd;");
    assert_eq!(t("abc\ndef", "<C-v>jlrx"), "xxc\nxxf");
    assert_eq!(t("one two", "wvecX<Esc>"), "one X");
    assert_eq!(t("f(a, b)", "fava(d"), "f");
    // o меняет конец выделения.
    let (mut e, mut c) = ed("abcdef");
    run(&mut e, &mut c, "llvlo");
    assert_eq!(e.cur, Pos::new(0, 2));
    assert_eq!(e.mode, Mode::Visual(VKind::Char));
    run(&mut e, &mut c, "<Esc>gv");
    assert_eq!(e.mode, Mode::Visual(VKind::Char));
}

#[test]
fn registers_and_macros() {
    assert_eq!(t("one\ntwo", "\"ayyj\"bdd\"ap"), "one\none");
    assert_eq!(t("a\nb", "\"ayyj\"Ayy\"aP"), "a\na\nb\nb");
    assert_eq!(t("x y", "\"_dwP"), "y");
    // Макрос: добавить «;» в конец трёх строк.
    assert_eq!(t("a\nb\nc", "qqA;<Esc>jq2@q"), "a;\nb;\nc;");
    assert_eq!(t("1\n2\n3\n4", "qwA!<Esc>jq@w@@"), "1!\n2!\n3!\n4");
    // Счётчик числа.
    assert_eq!(t("x = 41;", "<C-a>"), "x = 42;");
    assert_eq!(t("x = 10;", "5<C-x>"), "x = 5;");
    assert_eq!(t("v = -1", "3<C-a>"), "v = 2");
    assert_eq!(t("0x0f", "<C-a>"), "0x10");
}

#[test]
fn search_and_star() {
    let (mut e, mut c) = ed("foo bar\nbaz foo\nfoo");
    run(&mut e, &mut c, "/foo<CR>");
    assert_eq!(e.cur, Pos::new(1, 4));
    run(&mut e, &mut c, "n");
    assert_eq!(e.cur, Pos::new(2, 0));
    run(&mut e, &mut c, "n");
    assert_eq!(e.cur, Pos::new(0, 0)); // по кругу
    run(&mut e, &mut c, "N");
    assert_eq!(e.cur, Pos::new(2, 0));
    run(&mut e, &mut c, "gg*");
    assert_eq!(e.cur, Pos::new(1, 4));
    run(&mut e, &mut c, "?ba<CR>");
    assert_eq!(e.cur, Pos::new(1, 0));
    run(&mut e, &mut c, "gg/\\<b.z<CR>");
    assert_eq!(e.cur, Pos::new(1, 0));
    assert_eq!(t("a foo b foo", "d/foo<CR>"), "foo b foo");
    assert_eq!(t("x\nfoo\nfoo", "dn"), "x\nfoo\nfoo"); // нет предыдущего поиска
                                                       // % и парные скобки.
    let (mut e, mut c) = ed("if (a[1] + f(x)) {");
    run(&mut e, &mut c, "f(%");
    assert_eq!(e.cur, Pos::new(0, 15));
}

#[test]
fn ex_commands() {
    assert_eq!(t("foo foo\nfoo", ":s/foo/bar/<CR>"), "bar foo\nfoo");
    assert_eq!(t("foo foo\nfoo", ":%s/foo/bar/g<CR>"), "bar bar\nbar");
    assert_eq!(t("a,b,c", ":s/,/\\r/g<CR>"), "a\nb\nc");
    assert_eq!(t("john smith", ":s/\\(\\w\\+\\) \\(\\w\\+\\)/\\u\\2, \\u\\1/<CR>"), "Smith, John");
    assert_eq!(t("a\n\nb\n\n\nc", ":g/^$/d<CR>"), "a\nb\nc");
    assert_eq!(t("x1\ny\nx2", ":v/x/d<CR>"), "x1\nx2");
    assert_eq!(t("1\n2\n3", ":g/^/m0<CR>"), "3\n2\n1");
    assert_eq!(t("c\na\nb", ":sort<CR>"), "a\nb\nc");
    assert_eq!(t("10\n9\n100", ":sort n<CR>"), "9\n10\n100");
    assert_eq!(t("a\nb\nc\nd", ":2,3d<CR>"), "a\nd");
    assert_eq!(t("a\nb\nc", ":1m$<CR>"), "b\nc\na");
    assert_eq!(t("a\nb\nc", ":3t0<CR>"), "c\na\nb\nc");
    assert_eq!(t("a\nb\nc", ":%j<CR>"), "a b c");
    assert_eq!(t("a\nb", ":%>|1<<CR>"), "a\n    b");
    assert_eq!(t("a\nb\nc", ":%normal A;<CR>"), "a;\nb;\nc;");
    assert_eq!(t("x\nTODO a\ny\nTODO b", ":g/TODO/normal dw<CR>"), "x\na\ny\nb");
    assert_eq!(t("a\nb\nc", "Vj:s/$/!/<CR>"), "a!\nb!\nc");
    assert_eq!(t("\tx", ":retab<CR>"), "    x");
    assert_eq!(t("foo bar foo", ":s/foo/X/g|s/bar/Y/<CR>"), "X Y X");
    // Переход к строке.
    let (mut e, mut c) = ed("a\nb\nc");
    run(&mut e, &mut c, ":3<CR>");
    assert_eq!(e.cur.line, 2);
    run(&mut e, &mut c, ":1<CR>:/c/<CR>");
    assert_eq!(e.cur.line, 2);
    // Ошибки.
    run(&mut e, &mut c, ":nosuchcmd<CR>");
    assert!(e.message.as_ref().unwrap().1);
    run(&mut e, &mut c, ":%s/zzz/y/<CR>");
    assert!(e.message.as_ref().unwrap().0.contains("E486"));
}

#[test]
fn set_options() {
    let (mut e, mut c) = ed("x");
    run(&mut e, &mut c, ":set nonumber ts=8 sw=2<CR>");
    {
        let o = &e.cfg.borrow().options;
        assert!(!o.number);
        assert_eq!(o.tabstop, 8);
        assert_eq!(o.shiftwidth, 2);
    }
    run(&mut e, &mut c, ":set number! ts+=2<CR>");
    assert!(e.cfg.borrow().options.number);
    assert_eq!(e.cfg.borrow().options.tabstop, 10);
    run(&mut e, &mut c, ":set ft=rust<CR>");
    assert_eq!(e.lang.map(|l| l.id), Some("rust"));
    run(&mut e, &mut c, ":set ts?<CR>");
    assert!(e.message.as_ref().unwrap().0.contains("tabstop=10"));
    run(&mut e, &mut c, ":set foo<CR>");
    assert!(e.message.as_ref().unwrap().0.contains("E518"));
    run(&mut e, &mut c, ":colorscheme dark<CR>");
    assert_eq!(e.cfg.borrow().scheme, "dark");
    run(&mut e, &mut c, ":inoremap jk <lt>Esc><CR>");
    run(&mut e, &mut c, "Aabcjkx");
    assert_eq!(e.buf.text(), "xab");
    assert_eq!(e.mode, Mode::Normal);
    // Незавершённая последовательность из сопоставления — обычные клавиши.
    run(&mut e, &mut c, "Ajx<Esc>");
    assert_eq!(e.buf.text(), "xabjx");
    run(&mut e, &mut c, ":nmap <Leader>d dd<CR>\\d");
    assert_eq!(e.buf.text(), "");
}

#[test]
fn files_and_encodings() {
    let dir = std::env::temp_dir().join(format!("tvim-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("a.rs");
    std::fs::write(&f, "fn main() {}\r\n").unwrap();
    let (mut e, mut c) = ed("");
    e.open(&f).unwrap();
    assert_eq!(e.buf.text(), "fn main() {}");
    assert!(e.crlf);
    assert_eq!(e.lang.map(|l| l.id), Some("rust"));
    run(&mut e, &mut c, "A // x<Esc>:w<CR>");
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "fn main() {} // x\r\n");
    assert!(!e.modified());
    // Windows-1251.
    let g = dir.join("ru.txt");
    std::fs::write(&g, tvim::encoding::encode("Привет", tvim::encoding::Encoding::Cp1251, false)).unwrap();
    e.open(&g).unwrap();
    assert_eq!(e.buf.text(), "Привет");
    run(&mut e, &mut c, "A!<Esc>:w<CR>");
    assert_eq!(tvim::encoding::decode(&std::fs::read(&g).unwrap()).text, "Привет!\n");
    assert_eq!(std::fs::read(&g).unwrap()[0], 0xCF);
    // :w в другой файл не меняет имя буфера, :e! перечитывает.
    run(&mut e, &mut c, &format!(":w {}<CR>", dir.join("copy.txt").display()));
    assert!(dir.join("copy.txt").exists());
    run(&mut e, &mut c, "dd:e!<CR>");
    assert_eq!(e.buf.text(), "Привет!");
    // :q на изменённом — ошибка E37.
    run(&mut e, &mut c, "x:q<CR>");
    assert!(e.message.as_ref().unwrap().0.contains("E37"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn readonly_help() {
    let (mut e, mut c) = ed("text");
    e.readonly = true;
    run(&mut e, &mut c, "dd");
    assert_eq!(e.buf.text(), "text");
    run(&mut e, &mut c, "ix<Esc>");
    assert_eq!(e.buf.text(), "text");
}
