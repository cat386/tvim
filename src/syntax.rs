//! Подсветка синтаксиса.
//!
//! Каждая строка раскрашивается отдельно, но с учётом **состояния** на её
//! начале ([`LState`]): многострочные комментарии `/* … */`, строки в
//! тройных кавычках, блоки кода в Markdown, теги HTML. Редактор хранит
//! состояние начала каждой строки и пересчитывает его только от первой
//! изменённой строки, поэтому подсветка работает быстро и на больших файлах.
//!
//! Большинство языков описываются данными ([`Lang`]: ключевые слова,
//! комментарии, кавычки, флаги). Для языков разметки (Markdown, HTML/XML,
//! INI/TOML, YAML, diff, Makefile, CSS) есть свои небольшие разборщики.

use crate::config::Elem;

/// Вид разборщика.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Обычный язык программирования (описывается данными).
    Generic,
    Markdown,
    /// HTML и XML.
    Markup,
    /// INI и TOML: `[секция]`, `ключ = значение`.
    Ini,
    Yaml,
    Diff,
    Make,
    Css,
}

/// Описание языка.
pub struct Lang {
    pub id: &'static str,
    pub name: &'static str,
    /// Расширения и имена файлов по умолчанию.
    pub patterns: &'static str,
    pub kind: Kind,
    pub keywords: &'static [&'static str],
    pub types: &'static [&'static str],
    pub constants: &'static [&'static str],
    pub line_comments: &'static [&'static str],
    pub block_comments: &'static [(&'static str, &'static str)],
    /// Вложенные блочные комментарии (Rust).
    pub nested: bool,
    /// Кавычки строк.
    pub strings: &'static [char],
    /// Кавычки, строки в которых могут продолжаться на следующей строке.
    pub multiline: &'static [char],
    /// Тройные кавычки `"""` / `'''` (Python, TOML, Kotlin).
    pub triple: bool,
    /// Экранирование обратной чертой.
    pub escape: bool,
    /// Ключевые слова без учёта регистра (Pascal, SQL, Batch).
    pub icase: bool,
    /// Директивы `#…` в начале строки (C/C++/C#).
    pub preproc: bool,
    /// Слово с заглавной буквы — тип (Rust, Java, C#…).
    pub upper_types: bool,
    /// Rust: времена жизни `'a`, сырые строки `r#"…"#`.
    pub rust: bool,
    /// `имя!` — макрос (Rust).
    pub macros_bang: bool,
    /// `$var`, `${…}`, `$(…)` — особое (shell, PHP, make, PowerShell).
    pub dollar_vars: bool,
    /// `@имя` — аннотация/декоратор.
    pub at_annot: bool,
    /// `#` начинает комментарий только в начале слова (shell).
    pub hash_needs_space: bool,
    /// JSON: строка перед `:` — ключ (подсвечивается как тип).
    pub key_strings: bool,
    /// Слова, после которых до конца строки — комментарий (`REM`).
    pub comment_words: &'static [&'static str],
    /// `%VAR%` — особое (Batch).
    pub percent_vars: bool,
}

const BASE: Lang = Lang {
    id: "",
    name: "",
    patterns: "",
    kind: Kind::Generic,
    keywords: &[],
    types: &[],
    constants: &[],
    line_comments: &[],
    block_comments: &[],
    nested: false,
    strings: &['"', '\''],
    multiline: &[],
    triple: false,
    escape: true,
    icase: false,
    preproc: false,
    upper_types: false,
    rust: false,
    macros_bang: false,
    dollar_vars: false,
    at_annot: false,
    hash_needs_space: false,
    key_strings: false,
    comment_words: &[],
    percent_vars: false,
};

const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "const",
    "continue",
    "default",
    "do",
    "else",
    "enum",
    "extern",
    "for",
    "goto",
    "if",
    "inline",
    "register",
    "restrict",
    "return",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "volatile",
    "while",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Generic",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
];
const C_TYPES: &[&str] = &[
    "void",
    "char",
    "short",
    "int",
    "long",
    "float",
    "double",
    "signed",
    "unsigned",
    "_Bool",
    "bool",
    "size_t",
    "ssize_t",
    "ptrdiff_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "intptr_t",
    "uintptr_t",
    "FILE",
    "wchar_t",
];
const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "asm",
    "auto",
    "break",
    "case",
    "catch",
    "class",
    "co_await",
    "co_return",
    "co_yield",
    "concept",
    "const",
    "consteval",
    "constexpr",
    "constinit",
    "const_cast",
    "continue",
    "decltype",
    "default",
    "delete",
    "do",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "final",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "not",
    "operator",
    "or",
    "override",
    "private",
    "protected",
    "public",
    "register",
    "reinterpret_cast",
    "requires",
    "return",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "using",
    "virtual",
    "volatile",
    "while",
];
const CPP_TYPES: &[&str] = &[
    "void",
    "bool",
    "char",
    "char8_t",
    "char16_t",
    "char32_t",
    "wchar_t",
    "short",
    "int",
    "long",
    "float",
    "double",
    "signed",
    "unsigned",
    "size_t",
    "std",
    "string",
    "vector",
    "map",
    "unordered_map",
    "set",
    "unique_ptr",
    "shared_ptr",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
];
const C_CONSTANTS: &[&str] = &["NULL", "true", "false", "nullptr", "EOF", "stdin", "stdout", "stderr"];

/// Все языки.
pub const LANGS: &[Lang] = &[
    Lang {
        id: "rust",
        name: "Rust",
        patterns: "rs",
        keywords: &[
            "as",
            "async",
            "await",
            "break",
            "const",
            "continue",
            "crate",
            "dyn",
            "else",
            "enum",
            "extern",
            "fn",
            "for",
            "if",
            "impl",
            "in",
            "let",
            "loop",
            "match",
            "mod",
            "move",
            "mut",
            "pub",
            "ref",
            "return",
            "self",
            "Self",
            "static",
            "struct",
            "super",
            "trait",
            "type",
            "union",
            "unsafe",
            "use",
            "where",
            "while",
            "yield",
            "macro_rules",
        ],
        types: &[
            "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64", "bool", "char", "str", "String",
            "Vec", "Option", "Result", "Box", "Rc", "Arc", "RefCell", "Cell", "HashMap", "HashSet", "BTreeMap",
        ],
        constants: &["true", "false", "None", "Some", "Ok", "Err"],
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        nested: true,
        strings: &['"', '\''],
        multiline: &['"'],
        upper_types: true,
        rust: true,
        macros_bang: true,
        ..BASE
    },
    Lang {
        id: "c",
        name: "C",
        patterns: "c h",
        keywords: C_KEYWORDS,
        types: C_TYPES,
        constants: C_CONSTANTS,
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        preproc: true,
        ..BASE
    },
    Lang {
        id: "cpp",
        name: "C++",
        patterns: "cpp cc cxx hpp hh hxx ino",
        keywords: CPP_KEYWORDS,
        types: CPP_TYPES,
        constants: C_CONSTANTS,
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        preproc: true,
        ..BASE
    },
    Lang {
        id: "csharp",
        name: "C#",
        patterns: "cs",
        keywords: &[
            "abstract",
            "as",
            "async",
            "await",
            "base",
            "break",
            "case",
            "catch",
            "checked",
            "class",
            "const",
            "continue",
            "default",
            "delegate",
            "do",
            "else",
            "enum",
            "event",
            "explicit",
            "extern",
            "finally",
            "fixed",
            "for",
            "foreach",
            "get",
            "goto",
            "if",
            "implicit",
            "in",
            "init",
            "interface",
            "internal",
            "is",
            "lock",
            "namespace",
            "new",
            "operator",
            "out",
            "override",
            "params",
            "private",
            "protected",
            "public",
            "readonly",
            "record",
            "ref",
            "return",
            "sealed",
            "set",
            "sizeof",
            "stackalloc",
            "static",
            "struct",
            "switch",
            "this",
            "throw",
            "try",
            "typeof",
            "unchecked",
            "unsafe",
            "using",
            "var",
            "virtual",
            "volatile",
            "when",
            "where",
            "while",
            "yield",
        ],
        types: &[
            "bool", "byte", "char", "decimal", "double", "dynamic", "float", "int", "long", "object", "sbyte", "short", "string", "uint", "ulong",
            "ushort", "void",
        ],
        constants: &["true", "false", "null"],
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        preproc: true,
        upper_types: true,
        ..BASE
    },
    Lang {
        id: "java",
        name: "Java",
        patterns: "java",
        keywords: &[
            "abstract",
            "assert",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "default",
            "do",
            "else",
            "enum",
            "extends",
            "final",
            "finally",
            "for",
            "goto",
            "if",
            "implements",
            "import",
            "instanceof",
            "interface",
            "native",
            "new",
            "package",
            "private",
            "protected",
            "public",
            "record",
            "return",
            "sealed",
            "static",
            "strictfp",
            "super",
            "switch",
            "synchronized",
            "this",
            "throw",
            "throws",
            "transient",
            "try",
            "var",
            "volatile",
            "while",
            "yield",
        ],
        types: &["boolean", "byte", "char", "double", "float", "int", "long", "short", "void"],
        constants: &["true", "false", "null"],
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        upper_types: true,
        at_annot: true,
        ..BASE
    },
    Lang {
        id: "kotlin",
        name: "Kotlin",
        patterns: "kt kts",
        keywords: &[
            "as",
            "break",
            "by",
            "catch",
            "class",
            "companion",
            "const",
            "constructor",
            "continue",
            "data",
            "do",
            "else",
            "enum",
            "finally",
            "for",
            "fun",
            "if",
            "import",
            "in",
            "init",
            "interface",
            "internal",
            "is",
            "lateinit",
            "object",
            "open",
            "override",
            "package",
            "private",
            "protected",
            "public",
            "return",
            "sealed",
            "super",
            "this",
            "throw",
            "try",
            "typealias",
            "val",
            "var",
            "when",
            "while",
        ],
        types: &["Int", "Long", "Short", "Byte", "Double", "Float", "Boolean", "Char", "String", "Unit", "Any", "Nothing", "List", "Map", "Set"],
        constants: &["true", "false", "null"],
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        nested: true,
        triple: true,
        upper_types: true,
        at_annot: true,
        dollar_vars: true,
        ..BASE
    },
    Lang {
        id: "javascript",
        name: "JavaScript",
        patterns: "js mjs cjs jsx",
        keywords: &[
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "debugger",
            "default",
            "delete",
            "do",
            "else",
            "export",
            "extends",
            "finally",
            "for",
            "from",
            "function",
            "get",
            "if",
            "import",
            "in",
            "instanceof",
            "let",
            "new",
            "of",
            "return",
            "set",
            "static",
            "super",
            "switch",
            "this",
            "throw",
            "try",
            "typeof",
            "var",
            "void",
            "while",
            "with",
            "yield",
        ],
        types: &["Array", "Object", "String", "Number", "Boolean", "Promise", "Map", "Set", "Date", "RegExp", "Error", "JSON", "Math", "console"],
        constants: &["true", "false", "null", "undefined", "NaN", "Infinity"],
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        strings: &['"', '\'', '`'],
        multiline: &['`'],
        ..BASE
    },
    Lang {
        id: "typescript",
        name: "TypeScript",
        patterns: "ts tsx mts",
        keywords: &[
            "abstract",
            "as",
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "declare",
            "default",
            "delete",
            "do",
            "else",
            "enum",
            "export",
            "extends",
            "finally",
            "for",
            "from",
            "function",
            "get",
            "if",
            "implements",
            "import",
            "in",
            "infer",
            "instanceof",
            "interface",
            "is",
            "keyof",
            "let",
            "namespace",
            "new",
            "of",
            "private",
            "protected",
            "public",
            "readonly",
            "return",
            "set",
            "static",
            "super",
            "switch",
            "this",
            "throw",
            "try",
            "type",
            "typeof",
            "var",
            "void",
            "while",
            "yield",
        ],
        types: &["any", "boolean", "never", "number", "object", "string", "symbol", "unknown", "bigint", "Array", "Promise", "Record", "Partial"],
        constants: &["true", "false", "null", "undefined", "NaN", "Infinity"],
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        strings: &['"', '\'', '`'],
        multiline: &['`'],
        upper_types: true,
        at_annot: true,
        ..BASE
    },
    Lang {
        id: "go",
        name: "Go",
        patterns: "go",
        keywords: &[
            "break",
            "case",
            "chan",
            "const",
            "continue",
            "default",
            "defer",
            "else",
            "fallthrough",
            "for",
            "func",
            "go",
            "goto",
            "if",
            "import",
            "interface",
            "map",
            "package",
            "range",
            "return",
            "select",
            "struct",
            "switch",
            "type",
            "var",
        ],
        types: &[
            "bool",
            "byte",
            "complex64",
            "complex128",
            "error",
            "float32",
            "float64",
            "int",
            "int8",
            "int16",
            "int32",
            "int64",
            "rune",
            "string",
            "uint",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "uintptr",
            "any",
        ],
        constants: &["true", "false", "nil", "iota"],
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        strings: &['"', '\'', '`'],
        multiline: &['`'],
        ..BASE
    },
    Lang {
        id: "python",
        name: "Python",
        patterns: "py pyw pyi",
        keywords: &[
            "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from",
            "global", "if", "import", "in", "is", "lambda", "match", "case", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
            "with", "yield",
        ],
        types: &["int", "float", "str", "bool", "list", "dict", "set", "tuple", "bytes", "object", "type", "self", "cls"],
        constants: &["True", "False", "None", "NotImplemented", "Ellipsis"],
        line_comments: &["#"],
        triple: true,
        upper_types: true,
        at_annot: true,
        ..BASE
    },
    Lang {
        id: "ruby",
        name: "Ruby",
        patterns: "rb rake gemspec Rakefile Gemfile",
        keywords: &[
            "alias",
            "and",
            "begin",
            "break",
            "case",
            "class",
            "def",
            "defined?",
            "do",
            "else",
            "elsif",
            "end",
            "ensure",
            "for",
            "if",
            "in",
            "module",
            "next",
            "not",
            "or",
            "redo",
            "rescue",
            "retry",
            "return",
            "self",
            "super",
            "then",
            "undef",
            "unless",
            "until",
            "when",
            "while",
            "yield",
            "require",
            "attr_accessor",
            "attr_reader",
            "puts",
        ],
        constants: &["true", "false", "nil"],
        line_comments: &["#"],
        block_comments: &[("=begin", "=end")],
        upper_types: true,
        ..BASE
    },
    Lang {
        id: "php",
        name: "PHP",
        patterns: "php phtml",
        keywords: &[
            "abstract",
            "and",
            "as",
            "break",
            "case",
            "catch",
            "class",
            "clone",
            "const",
            "continue",
            "declare",
            "default",
            "do",
            "echo",
            "else",
            "elseif",
            "enum",
            "extends",
            "final",
            "finally",
            "fn",
            "for",
            "foreach",
            "function",
            "global",
            "if",
            "implements",
            "include",
            "instanceof",
            "interface",
            "match",
            "namespace",
            "new",
            "or",
            "print",
            "private",
            "protected",
            "public",
            "readonly",
            "require",
            "require_once",
            "return",
            "static",
            "switch",
            "throw",
            "trait",
            "try",
            "use",
            "var",
            "while",
            "yield",
        ],
        types: &["int", "float", "string", "bool", "array", "object", "mixed", "void", "self"],
        constants: &["true", "false", "null", "TRUE", "FALSE", "NULL"],
        line_comments: &["//", "#"],
        block_comments: &[("/*", "*/")],
        dollar_vars: true,
        ..BASE
    },
    Lang {
        id: "lua",
        name: "Lua",
        patterns: "lua",
        keywords: &[
            "and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if", "in", "local", "not", "or", "repeat", "return", "then",
            "until", "while",
        ],
        types: &["string", "table", "math", "io", "os", "coroutine"],
        constants: &["true", "false", "nil"],
        line_comments: &["--"],
        block_comments: &[("--[[", "]]"), ("[[", "]]")],
        ..BASE
    },
    Lang {
        id: "shell",
        name: "Shell (sh, bash)",
        patterns: "sh bash zsh ksh bashrc profile .bashrc .profile .zshrc PKGBUILD",
        keywords: &[
            "if", "then", "else", "elif", "fi", "case", "esac", "for", "while", "until", "do", "done", "in", "function", "select", "return", "exit",
            "break", "continue", "local", "export", "readonly", "declare", "source", "alias", "unset", "shift", "trap", "eval", "exec", "set",
        ],
        types: &["echo", "printf", "read", "cd", "test", "cat", "grep", "sed", "awk", "ls", "rm", "cp", "mv", "mkdir", "find", "xargs"],
        constants: &["true", "false"],
        line_comments: &["#"],
        strings: &['"', '\'', '`'],
        multiline: &['"', '\''],
        dollar_vars: true,
        hash_needs_space: true,
        ..BASE
    },
    Lang {
        id: "batch",
        name: "Batch (cmd)",
        patterns: "bat cmd",
        keywords: &[
            "if",
            "else",
            "for",
            "in",
            "do",
            "goto",
            "call",
            "exit",
            "set",
            "setlocal",
            "endlocal",
            "echo",
            "not",
            "exist",
            "defined",
            "errorlevel",
            "shift",
            "pushd",
            "popd",
            "cd",
            "start",
            "pause",
            "equ",
            "neq",
            "lss",
            "leq",
            "gtr",
            "geq",
        ],
        constants: &["on", "off", "nul"],
        line_comments: &["::"],
        strings: &['"'],
        escape: false,
        icase: true,
        comment_words: &["rem"],
        percent_vars: true,
        ..BASE
    },
    Lang {
        id: "powershell",
        name: "PowerShell",
        patterns: "ps1 psm1 psd1",
        keywords: &[
            "begin",
            "break",
            "catch",
            "class",
            "continue",
            "data",
            "do",
            "dynamicparam",
            "else",
            "elseif",
            "end",
            "enum",
            "exit",
            "filter",
            "finally",
            "for",
            "foreach",
            "function",
            "if",
            "in",
            "param",
            "process",
            "return",
            "switch",
            "throw",
            "trap",
            "try",
            "until",
            "using",
            "while",
        ],
        constants: &["$true", "$false", "$null"],
        line_comments: &["#"],
        block_comments: &[("<#", "#>")],
        escape: false,
        icase: true,
        dollar_vars: true,
        ..BASE
    },
    Lang {
        id: "sql",
        name: "SQL",
        patterns: "sql",
        keywords: &[
            "select",
            "from",
            "where",
            "and",
            "or",
            "not",
            "insert",
            "into",
            "values",
            "update",
            "set",
            "delete",
            "create",
            "table",
            "drop",
            "alter",
            "add",
            "index",
            "view",
            "join",
            "inner",
            "left",
            "right",
            "outer",
            "full",
            "on",
            "as",
            "group",
            "by",
            "order",
            "having",
            "limit",
            "offset",
            "union",
            "all",
            "distinct",
            "in",
            "is",
            "like",
            "between",
            "exists",
            "case",
            "when",
            "then",
            "else",
            "end",
            "primary",
            "key",
            "foreign",
            "references",
            "default",
            "unique",
            "constraint",
            "begin",
            "commit",
            "rollback",
            "transaction",
            "asc",
            "desc",
            "with",
            "returning",
            "if",
        ],
        types: &[
            "int",
            "integer",
            "bigint",
            "smallint",
            "real",
            "float",
            "double",
            "decimal",
            "numeric",
            "char",
            "varchar",
            "text",
            "date",
            "time",
            "timestamp",
            "boolean",
            "blob",
        ],
        constants: &["null", "true", "false"],
        line_comments: &["--"],
        block_comments: &[("/*", "*/")],
        strings: &['\'', '"'],
        escape: false,
        icase: true,
        ..BASE
    },
    Lang {
        id: "pascal",
        name: "Pascal / Delphi",
        patterns: "pas pp dpr lpr inc",
        keywords: &[
            "and",
            "array",
            "as",
            "asm",
            "begin",
            "case",
            "class",
            "const",
            "constructor",
            "destructor",
            "div",
            "do",
            "downto",
            "else",
            "end",
            "except",
            "exports",
            "file",
            "finalization",
            "finally",
            "for",
            "function",
            "goto",
            "if",
            "implementation",
            "in",
            "inherited",
            "initialization",
            "inline",
            "interface",
            "is",
            "label",
            "library",
            "mod",
            "nil",
            "not",
            "object",
            "of",
            "or",
            "out",
            "packed",
            "procedure",
            "program",
            "property",
            "raise",
            "record",
            "repeat",
            "set",
            "shl",
            "shr",
            "then",
            "to",
            "try",
            "type",
            "unit",
            "until",
            "uses",
            "var",
            "while",
            "with",
            "xor",
            "private",
            "public",
            "protected",
            "published",
            "override",
            "virtual",
            "overload",
        ],
        types: &[
            "integer", "cardinal", "byte", "word", "longint", "int64", "real", "double", "single", "extended", "boolean", "char", "string",
            "pointer", "variant",
        ],
        constants: &["true", "false", "nil"],
        line_comments: &["//"],
        block_comments: &[("{", "}"), ("(*", "*)")],
        strings: &['\''],
        escape: false,
        icase: true,
        ..BASE
    },
    Lang {
        id: "json",
        name: "JSON",
        patterns: "json jsonc json5 tvf tvui tvtheme",
        constants: &["true", "false", "null"],
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        strings: &['"'],
        key_strings: true,
        ..BASE
    },
    Lang {
        id: "toml",
        name: "TOML",
        patterns: "toml Cargo.lock",
        kind: Kind::Ini,
        constants: &["true", "false"],
        line_comments: &["#"],
        triple: true,
        ..BASE
    },
    Lang {
        id: "ini",
        name: "INI / conf",
        patterns: "ini cfg conf desktop properties editorconfig .editorconfig .gitconfig",
        kind: Kind::Ini,
        constants: &["true", "false", "yes", "no", "on", "off"],
        line_comments: &[";", "#"],
        strings: &['"'],
        escape: false,
        icase: true,
        ..BASE
    },
    Lang {
        id: "yaml",
        name: "YAML",
        patterns: "yaml yml",
        kind: Kind::Yaml,
        constants: &["true", "false", "null", "yes", "no", "on", "off", "~"],
        line_comments: &["#"],
        hash_needs_space: true,
        ..BASE
    },
    Lang { id: "markdown", name: "Markdown", patterns: "md markdown mkd", kind: Kind::Markdown, ..BASE },
    Lang { id: "html", name: "HTML", patterns: "html htm xhtml vue svelte", kind: Kind::Markup, ..BASE },
    Lang { id: "xml", name: "XML", patterns: "xml xsd xsl xslt svg csproj vcxproj props targets plist", kind: Kind::Markup, ..BASE },
    Lang {
        id: "css",
        name: "CSS / SCSS",
        patterns: "css scss less",
        kind: Kind::Css,
        keywords: &["important", "media", "import", "keyframes", "font-face", "supports", "charset", "include", "mixin", "extend", "use"],
        constants: &[
            "none",
            "auto",
            "inherit",
            "initial",
            "unset",
            "solid",
            "dashed",
            "bold",
            "normal",
            "block",
            "inline",
            "flex",
            "grid",
            "absolute",
            "relative",
            "fixed",
            "hidden",
            "center",
            "transparent",
        ],
        block_comments: &[("/*", "*/")],
        line_comments: &["//"],
        ..BASE
    },
    Lang {
        id: "make",
        name: "Makefile",
        patterns: "mk mak Makefile makefile GNUmakefile",
        kind: Kind::Make,
        keywords: &["ifeq", "ifneq", "ifdef", "ifndef", "else", "endif", "include", "define", "endef", "export", "override", "vpath", ".PHONY"],
        line_comments: &["#"],
        strings: &['"', '\''],
        dollar_vars: true,
        ..BASE
    },
    Lang { id: "diff", name: "Diff / Patch", patterns: "diff patch rej", kind: Kind::Diff, ..BASE },
    Lang {
        id: "cmake",
        name: "CMake",
        patterns: "cmake CMakeLists.txt",
        keywords: &[
            "if",
            "else",
            "elseif",
            "endif",
            "foreach",
            "endforeach",
            "while",
            "endwhile",
            "function",
            "endfunction",
            "macro",
            "endmacro",
            "set",
            "unset",
            "project",
            "add_executable",
            "add_library",
            "target_link_libraries",
            "target_include_directories",
            "find_package",
            "include",
            "option",
            "message",
            "install",
            "cmake_minimum_required",
            "add_subdirectory",
            "list",
            "string",
        ],
        constants: &["ON", "OFF", "TRUE", "FALSE", "PUBLIC", "PRIVATE", "INTERFACE", "REQUIRED"],
        line_comments: &["#"],
        strings: &['"'],
        dollar_vars: true,
        icase: true,
        ..BASE
    },
];

/// Язык по идентификатору.
pub fn lang_by_id(id: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|l| l.id == id)
}

/// Состояние разбора на начале строки.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LState {
    #[default]
    Normal,
    /// Блочный комментарий: номер пары в `block_comments` и глубина.
    Block(u8, u8),
    /// Строка: кавычка и признак тройных кавычек.
    Str(char, bool),
    /// Сырая строка Rust с указанным числом `#`.
    Raw(u8),
    /// Блок кода Markdown (```).
    Fence,
    /// Комментарий `<!-- … -->`.
    MarkupComment,
    /// Внутри тега HTML/XML.
    Tag,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn starts(s: &[char], i: usize, pat: &str) -> bool {
    let mut k = i;
    for p in pat.chars() {
        if k >= s.len() || s[k] != p {
            return false;
        }
        k += 1;
    }
    true
}

fn starts_icase(s: &[char], i: usize, pat: &str) -> bool {
    let mut k = i;
    for p in pat.chars() {
        if k >= s.len() || !s[k].to_lowercase().eq(p.to_lowercase()) {
            return false;
        }
        k += 1;
    }
    true
}

fn mark(out: &mut [Elem], a: usize, b: usize, e: Elem) {
    let b = b.min(out.len());
    if a < b {
        for x in &mut out[a..b] {
            *x = e;
        }
    }
}

/// Слова-пометки в комментариях.
const TODO_WORDS: &[&str] = &["TODO", "FIXME", "XXX", "NOTE", "HACK", "BUG"];

fn mark_todo(s: &[char], out: &mut [Elem], a: usize, b: usize) {
    let mut i = a;
    while i < b {
        if s[i].is_ascii_uppercase() && (i == 0 || !is_ident(s[i - 1])) {
            let mut j = i;
            while j < b && is_ident(s[j]) {
                j += 1;
            }
            let w: String = s[i..j].iter().collect();
            if TODO_WORDS.contains(&w.as_str()) {
                mark(out, i, j, Elem::Todo);
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
}

fn first_non_space(s: &[char]) -> Option<usize> {
    s.iter().position(|c| !c.is_whitespace())
}

fn word_in(list: &[&str], w: &str, icase: bool) -> bool {
    if icase {
        list.iter().any(|k| k.eq_ignore_ascii_case(w))
    } else {
        list.contains(&w)
    }
}

/// Раскрасить строку `s`, начав с состояния `st`. `out` получает цвет
/// каждого символа; возвращается состояние на начале следующей строки.
pub fn highlight(lang: &Lang, s: &[char], st: LState, out: &mut Vec<Elem>) -> LState {
    out.clear();
    out.resize(s.len(), Elem::Normal);
    match lang.kind {
        Kind::Generic => generic(lang, s, 0, st, out),
        Kind::Markdown => markdown(s, st, out),
        Kind::Markup => markup(s, st, out),
        Kind::Ini => ini(lang, s, st, out),
        Kind::Yaml => yaml(lang, s, st, out),
        Kind::Diff => {
            diff(s, out);
            LState::Normal
        }
        Kind::Make => make(lang, s, st, out),
        Kind::Css => css(lang, s, st, out),
    }
}

/// Разбор обычного языка с позиции `from`.
fn generic(l: &Lang, s: &[char], from: usize, mut st: LState, out: &mut [Elem]) -> LState {
    let n = s.len();
    let mut i = from;
    let first_ns = first_non_space(s);
    // Начало текущей строки-литерала (для ключей JSON).
    let mut str_start = 0usize;
    'outer: loop {
        // ---- продолжение многострочных конструкций ----
        match st {
            LState::Block(bi, depth) => {
                let (open, close) = l.block_comments[bi as usize];
                let start = i;
                let mut d = depth;
                while i < n {
                    if l.nested && starts(s, i, open) {
                        d += 1;
                        i += open.chars().count();
                        continue;
                    }
                    if starts(s, i, close) {
                        i += close.chars().count();
                        d -= 1;
                        if d == 0 {
                            break;
                        }
                        continue;
                    }
                    i += 1;
                }
                mark(out, start, i, Elem::Comment);
                mark_todo(s, out, start, i.min(n));
                if d > 0 {
                    return LState::Block(bi, d);
                }
                st = LState::Normal;
            }
            LState::Str(q, triple) => {
                let start = i;
                let mut closed = false;
                while i < n {
                    if l.escape && s[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if s[i] == q {
                        if triple {
                            if i + 2 < n && s[i + 1] == q && s[i + 2] == q {
                                i += 3;
                                closed = true;
                                break;
                            }
                            i += 1;
                            continue;
                        }
                        i += 1;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                let i2 = i.min(n);
                mark(out, start, i2, Elem::String);
                if l.escape {
                    let mut k = start;
                    while k < i2 {
                        if s[k] == '\\' {
                            mark(out, k, k + 2, Elem::Special);
                            k += 2;
                        } else {
                            k += 1;
                        }
                    }
                }
                if !closed {
                    let cont = triple || l.multiline.contains(&q) || (n > 0 && s[n - 1] == '\\' && l.escape);
                    return if cont { LState::Str(q, triple) } else { LState::Normal };
                }
                if l.key_strings {
                    let mut j = i2;
                    while j < n && s[j].is_whitespace() {
                        j += 1;
                    }
                    if j < n && s[j] == ':' {
                        mark(out, str_start, i2, Elem::Type);
                    }
                }
                i = i2;
                st = LState::Normal;
            }
            LState::Raw(h) => {
                let start = i;
                let mut closed = false;
                while i < n {
                    if s[i] == '"' && (1..=h as usize).all(|k| i + k < n && s[i + k] == '#') {
                        i += 1 + h as usize;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                mark(out, start, i, Elem::String);
                if !closed {
                    return LState::Raw(h);
                }
                st = LState::Normal;
            }
            _ => {}
        }
        if i >= n {
            return st;
        }
        let c = s[i];

        // ---- комментарии ----
        for lc in l.line_comments {
            if starts(s, i, lc) && (!l.hash_needs_space || i == 0 || s[i - 1].is_whitespace()) {
                // В Lua «--[[» — это блочный комментарий, проверим его ниже.
                if l.block_comments.iter().any(|(o, _)| o.starts_with(lc) && o.len() > lc.len() && starts(s, i, o)) {
                    break;
                }
                mark(out, i, n, Elem::Comment);
                mark_todo(s, out, i, n);
                return LState::Normal;
            }
        }
        for (bi, (open, _)) in l.block_comments.iter().enumerate() {
            let hit = if l.icase { starts_icase(s, i, open) } else { starts(s, i, open) };
            // «=begin» в Ruby — только в начале строки.
            if hit && (!open.starts_with('=') || i == 0) {
                let len = open.chars().count();
                mark(out, i, i + len, Elem::Comment);
                i += len;
                st = LState::Block(bi as u8, 1);
                continue 'outer;
            }
        }

        // ---- директивы препроцессора ----
        if l.preproc && c == '#' && Some(i) == first_ns {
            let mut j = i + 1;
            while j < n && s[j] == ' ' {
                j += 1;
            }
            let ws = j;
            while j < n && is_ident(s[j]) {
                j += 1;
            }
            let word: String = s[ws..j].iter().collect();
            mark(out, i, j, Elem::Preproc);
            i = j;
            if word == "include" {
                while i < n && s[i] == ' ' {
                    i += 1;
                }
                if i < n && s[i] == '<' {
                    let end = s[i..].iter().position(|&x| x == '>').map(|p| i + p + 1).unwrap_or(n);
                    mark(out, i, end, Elem::String);
                    i = end;
                }
            }
            continue;
        }

        // ---- строки ----
        if l.triple && (c == '"' || c == '\'') && i + 2 < n && s[i + 1] == c && s[i + 2] == c {
            mark(out, i, i + 3, Elem::String);
            str_start = i;
            i += 3;
            st = LState::Str(c, true);
            continue;
        }
        if l.rust && (c == 'r' || (c == 'b' && i + 1 < n && s[i + 1] == 'r')) && (i == 0 || !is_ident(s[i - 1])) {
            let mut j = i + if c == 'b' { 2 } else { 1 };
            let mut h = 0u8;
            while j < n && s[j] == '#' {
                h += 1;
                j += 1;
            }
            if j < n && s[j] == '"' {
                mark(out, i, j + 1, Elem::String);
                i = j + 1;
                st = LState::Raw(h);
                continue;
            }
        }
        if l.strings.contains(&c) {
            if c == '\'' && l.rust {
                // Символ 'x', '\n' или время жизни 'a.
                if i + 1 < n && s[i + 1] == '\\' {
                    let end = (i + 2..n.min(i + 12)).find(|&k| s[k] == '\'').map(|k| k + 1).unwrap_or(i + 2);
                    mark(out, i, end, Elem::String);
                    mark(out, i + 1, end.saturating_sub(1), Elem::Special);
                    i = end;
                    continue;
                }
                if i + 2 < n && s[i + 2] == '\'' {
                    mark(out, i, i + 3, Elem::String);
                    i += 3;
                    continue;
                }
                let mut j = i + 1;
                while j < n && is_ident(s[j]) {
                    j += 1;
                }
                mark(out, i, j, Elem::Special);
                i = j.max(i + 1);
                continue;
            }
            mark(out, i, i + 1, Elem::String);
            str_start = i;
            i += 1;
            st = LState::Str(c, false);
            continue;
        }

        // ---- числа ----
        if c.is_ascii_digit() && (i == 0 || !is_ident(s[i - 1])) {
            let hex = c == '0' && i + 1 < n && matches!(s[i + 1], 'x' | 'X');
            let mut j = i;
            while j < n {
                let d = s[j];
                let ok = d.is_ascii_alphanumeric()
                    || d == '_'
                    || (d == '.' && j + 1 < n && s[j + 1].is_ascii_digit())
                    || ((d == '+' || d == '-') && !hex && j > i && matches!(s[j - 1], 'e' | 'E'));
                if !ok {
                    break;
                }
                j += 1;
            }
            mark(out, i, j, Elem::Number);
            i = j;
            continue;
        }

        // ---- переменные $x, %x% ----
        if l.dollar_vars && c == '$' && i + 1 < n {
            let mut j = i + 1;
            match s[j] {
                '{' | '(' => {
                    let close = if s[j] == '{' { '}' } else { ')' };
                    j = s[j..].iter().position(|&x| x == close).map(|p| j + p + 1).unwrap_or(n);
                }
                x if is_ident(x) => {
                    while j < n && is_ident(s[j]) {
                        j += 1;
                    }
                }
                _ => j += 1,
            }
            // В PowerShell $true/$false — константы.
            let w: String = s[i..j].iter().collect();
            let e = if word_in(l.constants, &w, true) { Elem::Constant } else { Elem::Special };
            mark(out, i, j, e);
            i = j;
            continue;
        }
        if l.percent_vars && c == '%' {
            if let Some(p) = s[i + 1..].iter().position(|&x| x == '%') {
                if p > 0 && s[i + 1..i + 1 + p].iter().all(|&x| is_ident(x) || x == '~') {
                    mark(out, i, i + p + 2, Elem::Special);
                    i += p + 2;
                    continue;
                }
            }
            if i + 1 < n && (s[i + 1].is_ascii_digit() || s[i + 1] == '%') {
                mark(out, i, i + 2, Elem::Special);
                i += 2;
                continue;
            }
        }
        if l.at_annot && c == '@' && i + 1 < n && (s[i + 1].is_alphabetic() || s[i + 1] == '_') {
            let mut j = i + 1;
            while j < n && (is_ident(s[j]) || s[j] == '.') {
                j += 1;
            }
            mark(out, i, j, Elem::Preproc);
            i = j;
            continue;
        }

        // ---- идентификаторы ----
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < n && is_ident(s[j]) {
                j += 1;
            }
            let w: String = s[i..j].iter().collect();
            if word_in(l.comment_words, &w, true) && (j == n || s[j].is_whitespace()) {
                mark(out, i, n, Elem::Comment);
                return LState::Normal;
            }
            let mut k = j;
            while k < n && s[k] == ' ' {
                k += 1;
            }
            let e = if word_in(l.keywords, &w, l.icase) {
                Elem::Keyword
            } else if word_in(l.types, &w, l.icase) {
                Elem::Type
            } else if word_in(l.constants, &w, l.icase) {
                Elem::Constant
            } else if l.macros_bang && j < n && s[j] == '!' && !(j + 1 < n && s[j + 1] == '=') {
                mark(out, i, j + 1, Elem::Special);
                i = j + 1;
                continue;
            } else if k < n && s[k] == '(' {
                Elem::Function
            } else if l.upper_types && c.is_uppercase() && s[i..j].iter().any(|x| x.is_lowercase()) {
                Elem::Type
            } else {
                Elem::Normal
            };
            mark(out, i, j, e);
            i = j;
            continue;
        }

        // ---- операторы ----
        if "+-*/%=<>!&|^~?:".contains(c) {
            mark(out, i, i + 1, Elem::Operator);
        }
        i += 1;
    }
}

/// Markdown.
fn markdown(s: &[char], st: LState, out: &mut [Elem]) -> LState {
    let n = s.len();
    let t = first_non_space(s).unwrap_or(n);
    let fence = starts(s, t, "```") || starts(s, t, "~~~");
    if st == LState::Fence {
        if fence {
            mark(out, 0, n, Elem::Preproc);
            return LState::Normal;
        }
        mark(out, 0, n, Elem::String);
        return LState::Fence;
    }
    if fence {
        mark(out, 0, n, Elem::Preproc);
        return LState::Fence;
    }
    if t >= n {
        return LState::Normal;
    }
    if s[t] == '#' {
        mark(out, 0, n, Elem::Keyword);
        return LState::Normal;
    }
    if s[t] == '>' {
        mark(out, 0, n, Elem::Comment);
        return LState::Normal;
    }
    let rest: String = s[t..].iter().filter(|c| !c.is_whitespace()).collect();
    if rest.len() >= 3 && (rest.chars().all(|c| c == '-') || rest.chars().all(|c| c == '*') || rest.chars().all(|c| c == '_')) {
        mark(out, 0, n, Elem::Preproc);
        return LState::Normal;
    }
    if t >= 4 && s[..t].iter().all(|&c| c == ' ') {
        // Код с отступом.
        mark(out, 0, n, Elem::String);
        return LState::Normal;
    }
    let mut i = t;
    if matches!(s[t], '-' | '*' | '+') && t + 1 < n && s[t + 1] == ' ' {
        mark(out, t, t + 1, Elem::Special);
        i = t + 2;
        if starts(s, i, "[ ]") || starts(s, i, "[x]") || starts(s, i, "[X]") {
            mark(out, i, i + 3, Elem::Special);
            i += 3;
        }
    } else {
        let mut j = t;
        while j < n && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > t && j < n && (s[j] == '.' || s[j] == ')') {
            mark(out, t, j + 1, Elem::Special);
            i = j + 1;
        }
    }
    // Разметка внутри строки.
    while i < n {
        let c = s[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if c == '`' {
            let end = s[i + 1..].iter().position(|&x| x == '`').map(|p| i + p + 2).unwrap_or(n);
            mark(out, i, end, Elem::String);
            i = end;
            continue;
        }
        if (starts(s, i, "**") || starts(s, i, "__")) && i + 2 < n {
            let pat: String = s[i..i + 2].iter().collect();
            if let Some(p) = (i + 2..n.saturating_sub(1)).find(|&k| starts(s, k, &pat)) {
                mark(out, i, p + 2, Elem::Type);
                i = p + 2;
                continue;
            }
        }
        if (c == '*' || c == '_') && (i == 0 || !s[i - 1].is_alphanumeric()) && i + 1 < n && !s[i + 1].is_whitespace() {
            if let Some(p) = (i + 1..n).find(|&k| s[k] == c) {
                mark(out, i, p + 1, Elem::Constant);
                i = p + 1;
                continue;
            }
        }
        if c == '[' {
            if let Some(p) = (i + 1..n).find(|&k| s[k] == ']') {
                if p + 1 < n && s[p + 1] == '(' {
                    let end = (p + 2..n).find(|&k| s[k] == ')').map(|k| k + 1).unwrap_or(n);
                    mark(out, i, p + 1, Elem::Function);
                    mark(out, p + 1, end, Elem::String);
                    i = end;
                    continue;
                }
            }
        }
        if c == '<' && (starts(s, i + 1, "http") || s.get(i + 1).map(|x| x.is_alphabetic() || *x == '/').unwrap_or(false)) {
            let end = (i + 1..n).find(|&k| s[k] == '>').map(|k| k + 1).unwrap_or(n);
            mark(out, i, end, Elem::Preproc);
            i = end;
            continue;
        }
        i += 1;
    }
    LState::Normal
}

/// HTML и XML.
fn markup(s: &[char], mut st: LState, out: &mut [Elem]) -> LState {
    let n = s.len();
    let mut i = 0;
    while i < n {
        match st {
            LState::MarkupComment => {
                let start = i;
                match (i..n).find(|&k| starts(s, k, "-->")) {
                    Some(p) => {
                        mark(out, start, p + 3, Elem::Comment);
                        i = p + 3;
                        st = LState::Normal;
                    }
                    None => {
                        mark(out, start, n, Elem::Comment);
                        return st;
                    }
                }
            }
            LState::Tag => {
                // Внутри тега: атрибуты, значения, конец «>».
                let c = s[i];
                if c == '>' || starts(s, i, "/>") || starts(s, i, "?>") {
                    let len = if c == '>' { 1 } else { 2 };
                    mark(out, i, i + len, Elem::Keyword);
                    i += len;
                    st = LState::Normal;
                } else if c == '"' || c == '\'' {
                    let end = s[i + 1..].iter().position(|&x| x == c).map(|p| i + p + 2).unwrap_or(n);
                    mark(out, i, end, Elem::String);
                    i = end;
                } else if c == '=' {
                    mark(out, i, i + 1, Elem::Operator);
                    i += 1;
                } else if c.is_alphabetic() || c == '_' || c == ':' || c == '@' {
                    let mut j = i;
                    while j < n && (is_ident(s[j]) || matches!(s[j], '-' | ':' | '.' | '@')) {
                        j += 1;
                    }
                    mark(out, i, j, Elem::Type);
                    i = j;
                } else {
                    i += 1;
                }
            }
            _ => {
                let c = s[i];
                if starts(s, i, "<!--") {
                    mark(out, i, i + 4, Elem::Comment);
                    i += 4;
                    st = LState::MarkupComment;
                } else if c == '<' && i + 1 < n && (s[i + 1].is_alphabetic() || matches!(s[i + 1], '/' | '!' | '?')) {
                    let mut j = i + 1;
                    while j < n && matches!(s[j], '/' | '!' | '?') {
                        j += 1;
                    }
                    while j < n && (is_ident(s[j]) || matches!(s[j], '-' | ':' | '.')) {
                        j += 1;
                    }
                    mark(out, i, j, Elem::Keyword);
                    i = j;
                    st = LState::Tag;
                } else if c == '&' {
                    let end = (i + 1..n.min(i + 12)).find(|&k| s[k] == ';').map(|k| k + 1);
                    match end {
                        Some(e) => {
                            mark(out, i, e, Elem::Special);
                            i = e;
                        }
                        None => i += 1,
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    st
}

/// INI и TOML.
fn ini(l: &Lang, s: &[char], st: LState, out: &mut [Elem]) -> LState {
    if st != LState::Normal {
        return generic(l, s, 0, st, out);
    }
    let n = s.len();
    let Some(t) = first_non_space(s) else { return LState::Normal };
    if l.line_comments.iter().any(|c| starts(s, t, c)) {
        mark(out, t, n, Elem::Comment);
        mark_todo(s, out, t, n);
        return LState::Normal;
    }
    if s[t] == '[' {
        let end = (t..n).rfind(|&k| s[k] == ']').map(|k| k + 1).unwrap_or(n);
        mark(out, t, end, Elem::Keyword);
        return generic(l, s, end, LState::Normal, out);
    }
    // Ключ до «=» (в INI — ещё и «:»), не внутри кавычек.
    let mut q: Option<char> = None;
    let mut eq = None;
    for k in t..n {
        let c = s[k];
        if let Some(qc) = q {
            if c == qc {
                q = None;
            }
        } else if c == '"' || c == '\'' {
            q = Some(c);
        } else if c == '=' || (c == ':' && l.id == "ini") {
            eq = Some(k);
            break;
        }
    }
    match eq {
        Some(e) => {
            let mut ke = e;
            while ke > t && s[ke - 1].is_whitespace() {
                ke -= 1;
            }
            mark(out, t, ke, Elem::Type);
            mark(out, e, e + 1, Elem::Operator);
            if l.id == "ini" {
                // В INI значение — просто текст; подсветим числа и константы.
                generic_value(l, s, e + 1, out);
                LState::Normal
            } else {
                generic(l, s, e + 1, LState::Normal, out)
            }
        }
        None => generic(l, s, t, LState::Normal, out),
    }
}

/// Значение INI: число или константа целиком, иначе — обычный текст.
fn generic_value(l: &Lang, s: &[char], from: usize, out: &mut [Elem]) {
    let v: String = s[from..].iter().collect();
    let t = v.trim();
    let a = from + v.len() - v.trim_start().len();
    let b = a + t.chars().count();
    if !t.is_empty() && (t.parse::<f64>().is_ok() || word_in(l.constants, t, true)) {
        mark(out, a, b, if t.parse::<f64>().is_ok() { Elem::Number } else { Elem::Constant });
    } else if t.starts_with('"') {
        mark(out, a, b, Elem::String);
    }
}

/// YAML.
fn yaml(l: &Lang, s: &[char], st: LState, out: &mut [Elem]) -> LState {
    if st != LState::Normal {
        return generic(l, s, 0, st, out);
    }
    let n = s.len();
    let Some(t) = first_non_space(s) else { return LState::Normal };
    if s[t] == '#' {
        mark(out, t, n, Elem::Comment);
        mark_todo(s, out, t, n);
        return LState::Normal;
    }
    if t == 0 && (starts(s, 0, "---") || starts(s, 0, "...")) {
        mark(out, 0, n, Elem::Preproc);
        return LState::Normal;
    }
    let mut i = t;
    while i < n && s[i] == '-' && (i + 1 == n || s[i + 1] == ' ') {
        mark(out, i, i + 1, Elem::Special);
        i += 1;
        while i < n && s[i] == ' ' {
            i += 1;
        }
    }
    // Ключ «имя:» (двоеточие перед пробелом или концом строки).
    if i < n && s[i] != '"' && s[i] != '\'' {
        let colon = (i..n).find(|&k| s[k] == ':' && (k + 1 == n || s[k + 1] == ' ') && !s[i..k].contains(&'#'));
        if let Some(k) = colon {
            mark(out, i, k, Elem::Type);
            mark(out, k, k + 1, Elem::Operator);
            i = k + 1;
        }
    }
    while i < n && s[i] == ' ' {
        i += 1;
    }
    // Якоря, ссылки и теги: &a, *a, !tag.
    if i < n && matches!(s[i], '&' | '*' | '!') && i + 1 < n && !s[i + 1].is_whitespace() {
        let mut j = i + 1;
        while j < n && !s[j].is_whitespace() {
            j += 1;
        }
        mark(out, i, j, Elem::Special);
        i = j;
    }
    if i < n && matches!(s[i], '|' | '>') {
        mark(out, i, i + 1, Elem::Operator);
        return LState::Normal;
    }
    generic(l, s, i, LState::Normal, out)
}

/// diff/patch.
fn diff(s: &[char], out: &mut [Elem]) {
    let n = s.len();
    let e = if starts(s, 0, "+++") || starts(s, 0, "---") || starts(s, 0, "diff ") || starts(s, 0, "index ") {
        Elem::Preproc
    } else if starts(s, 0, "@@") {
        Elem::Keyword
    } else if starts(s, 0, "+") {
        Elem::String
    } else if starts(s, 0, "-") {
        Elem::Special
    } else {
        Elem::Normal
    };
    mark(out, 0, n, e);
}

/// Makefile.
fn make(l: &Lang, s: &[char], st: LState, out: &mut [Elem]) -> LState {
    let n = s.len();
    if n > 0 && s[0] == '\t' {
        // Команда рецепта.
        return generic(l, s, 0, st, out);
    }
    let line: String = s.iter().collect();
    // Присваивание: VAR = …, VAR := …, VAR ?= …, VAR += ….
    if let Some(p) = line.find('=') {
        let before = &line[..p];
        let name = before.trim_end_matches([':', '?', '+', '!']).trim();
        if !name.is_empty() && name.chars().all(|c| is_ident(c) || c == '.' || c == '-') && !word_in(l.keywords, name, false) {
            let a = line[..line.len() - line.trim_start().len()].chars().count();
            let b = a + name.chars().count();
            mark(out, a, b, Elem::Type);
            return generic(l, s, b, st, out);
        }
    }
    // Цель: «имя: зависимости».
    if let Some(p) = s.iter().position(|&c| c == ':') {
        if !s[..p].contains(&'#') && !s[..p].contains(&'=') && !(p + 1 < n && s[p + 1] == '=') {
            let t = first_non_space(s).unwrap_or(0);
            let w: String = s[t..p].iter().collect();
            if !word_in(l.keywords, w.split_whitespace().next().unwrap_or(""), false) {
                mark(out, t, p, Elem::Function);
                mark(out, p, p + 1, Elem::Operator);
                return generic(l, s, p + 1, st, out);
            }
        }
    }
    generic(l, s, 0, st, out)
}

/// CSS.
fn css(l: &Lang, s: &[char], st: LState, out: &mut [Elem]) -> LState {
    let r = generic(l, s, 0, st, out);
    if st != LState::Normal {
        return r;
    }
    let n = s.len();
    let selector = s.contains(&'{');
    let t = first_non_space(s).unwrap_or(n);
    let mut i = 0;
    while i < n {
        if out[i] == Elem::Comment || out[i] == Elem::String {
            i += 1;
            continue;
        }
        let c = s[i];
        if c == '#' && i + 1 < n && s[i + 1].is_ascii_hexdigit() && !selector {
            let mut j = i + 1;
            while j < n && s[j].is_ascii_alphanumeric() {
                j += 1;
            }
            mark(out, i, j, Elem::Number);
            i = j;
            continue;
        }
        if selector && (c == '.' || c == '#') && i + 1 < n && (s[i + 1].is_alphabetic() || s[i + 1] == '_' || s[i + 1] == '-') {
            let mut j = i + 1;
            while j < n && (is_ident(s[j]) || s[j] == '-') {
                j += 1;
            }
            mark(out, i, j, Elem::Function);
            i = j;
            continue;
        }
        if c == '@' {
            let mut j = i + 1;
            while j < n && (is_ident(s[j]) || s[j] == '-') {
                j += 1;
            }
            mark(out, i, j, Elem::Preproc);
            i = j;
            continue;
        }
        i += 1;
    }
    // Свойство: «имя:» в начале строки без «{».
    if !selector && t < n && (s[t].is_alphabetic() || s[t] == '-') {
        let mut j = t;
        while j < n && (is_ident(s[j]) || s[j] == '-') {
            j += 1;
        }
        let mut k = j;
        while k < n && s[k] == ' ' {
            k += 1;
        }
        if k < n && s[k] == ':' {
            mark(out, t, j, Elem::Type);
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hl(id: &str, line: &str, st: LState) -> (Vec<Elem>, LState) {
        let s: Vec<char> = line.chars().collect();
        let mut out = Vec::new();
        let r = highlight(lang_by_id(id).unwrap(), &s, st, &mut out);
        (out, r)
    }

    #[test]
    fn rust_tokens() {
        let (o, st) = hl("rust", "fn main() { let s = \"hi\\n\"; // TODO x", LState::Normal);
        assert_eq!(st, LState::Normal);
        assert_eq!(o[0], Elem::Keyword); // fn
        assert_eq!(o[3], Elem::Function); // main
        assert_eq!(o[12], Elem::Keyword); // let
        assert_eq!(o[20], Elem::String);
        assert_eq!(o[23], Elem::Special); // \n
        assert_eq!(o[28], Elem::Comment);
        assert_eq!(o[31], Elem::Todo);
        let (o, _) = hl("rust", "println!(\"{}\", 'a', x: &'a str, 42u8, Foo::new())", LState::Normal);
        assert_eq!(o[0], Elem::Special);
        assert_eq!(o[15], Elem::String); // 'a'
        assert_eq!(o[24], Elem::Special); // 'a lifetime
        assert_eq!(o[33], Elem::Number);
        assert_eq!(o[38], Elem::Type); // Foo
    }

    #[test]
    fn multiline_states() {
        let (o, st) = hl("c", "int x; /* start", LState::Normal);
        assert_eq!(o[0], Elem::Type);
        assert_eq!(st, LState::Block(0, 1));
        let (o, st) = hl("c", "end */ return 0;", st);
        assert_eq!(o[0], Elem::Comment);
        assert_eq!(o[7], Elem::Keyword);
        assert_eq!(st, LState::Normal);
        let (_, st) = hl("python", "x = \"\"\"doc", LState::Normal);
        assert_eq!(st, LState::Str('"', true));
        let (o, st) = hl("python", "end\"\"\" + 1", st);
        assert_eq!(o[0], Elem::String);
        assert_eq!(o[9], Elem::Number);
        assert_eq!(st, LState::Normal);
        let (_, st) = hl("rust", "/* a /* b */", LState::Normal);
        assert_eq!(st, LState::Block(0, 1));
    }

    #[test]
    fn markup_and_data() {
        let (o, _) = hl("json", "  \"key\": [1, true, \"v\"]", LState::Normal);
        assert_eq!(o[2], Elem::Type);
        assert_eq!(o[10], Elem::Number);
        assert_eq!(o[13], Elem::Constant);
        assert_eq!(o[19], Elem::String);
        let (o, st) = hl("html", "<a href=\"x\">text &amp; <!-- c", LState::Normal);
        assert_eq!(o[0], Elem::Keyword);
        assert_eq!(o[3], Elem::Type);
        assert_eq!(o[8], Elem::String);
        assert_eq!(o[12], Elem::Normal);
        assert_eq!(o[17], Elem::Special);
        assert_eq!(st, LState::MarkupComment);
        let (o, _) = hl("toml", "name = \"tvim\" # c", LState::Normal);
        assert_eq!(o[0], Elem::Type);
        assert_eq!(o[7], Elem::String);
        assert_eq!(o[14], Elem::Comment);
        let (o, st) = hl("markdown", "# Title", LState::Normal);
        assert_eq!(o[2], Elem::Keyword);
        assert_eq!(st, LState::Normal);
        let (_, st) = hl("markdown", "```rust", LState::Normal);
        assert_eq!(st, LState::Fence);
        let (o, _) = hl("yaml", "- name: \"x\" # c", LState::Normal);
        assert_eq!(o[0], Elem::Special);
        assert_eq!(o[2], Elem::Type);
        assert_eq!(o[8], Elem::String);
        assert_eq!(o[12], Elem::Comment);
        let (o, _) = hl("shell", "echo $HOME # c", LState::Normal);
        assert_eq!(o[5], Elem::Special);
        assert_eq!(o[11], Elem::Comment);
        let (o, _) = hl("batch", "REM hello", LState::Normal);
        assert_eq!(o[5], Elem::Comment);
        let (o, _) = hl("make", "all: main.o", LState::Normal);
        assert_eq!(o[0], Elem::Function);
    }

    #[test]
    fn all_langs_survive() {
        let samples = ["", "x", "'", "\"", "/*", "#", "<", "[", "`", "${", "%", "@", "-- [[", "r#\"", "a: b", "\\"];
        for l in LANGS {
            for s in samples {
                let mut st = LState::Normal;
                for _ in 0..3 {
                    let (_, n) = hl(l.id, s, st);
                    st = n;
                }
            }
        }
    }
}
