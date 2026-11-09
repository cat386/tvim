//! TVim — текстовый редактор в духе vim на tvision-rs.
//!
//! Запуск: `cargo run -p tvim -- [файлы...]`

fn main() -> std::io::Result<()> {
    let files: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    if files.iter().any(|f| f == "--help" || f == "-h") {
        println!("TVim — текстовый редактор в духе vim на tvision-rs\n\nИспользование: tvim [файлы...]\n\nВнутри: :help или F1 — справка, F9 — параметры, :q — выход.");
        return Ok(());
    }
    tvim::app::run(&files)
}
