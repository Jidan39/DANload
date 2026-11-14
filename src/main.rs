//! DANload - titik masuk aplikasi.
//!
//! Pembagian modul:
//! - `app`        : penghubung tampilan (Slint) dengan logika
//! - `config`     : pengaturan yang disimpan di config.json
//! - `deps`       : mencari yt-dlp, FFmpeg, ffprobe, dan Deno
//! - `downloader` : menjalankan yt-dlp untuk satu unduhan dan membaca kemajuannya
//! - `formats`    : pilihan format (MP4/MP3) dan argumen yt-dlp-nya
//! - `platform`   : platform terpilih (YouTube/Facebook/Instagram) dan pengecekan URL
//! - `process`    : menjalankan program eksternal tanpa console, prioritas rendah, kill tree
//! - `setup`      : mengunduh dan memasang komponen (dipakai wizard setup)

// Di build rilis, jangan munculkan jendela console hitam di belakang aplikasi.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod deps;
mod downloader;
mod formats;
mod platform;
mod process;
mod setup;

// Memasukkan kode Rust hasil kompilasi file `.slint` (dibuat oleh build.rs).
// Dari sinilah tipe `AppWindow`, `ComponentInfo`, `WizardState`, dan lainnya berasal.
slint::include_modules!();

fn main() {
    if let Err(error) = run() {
        // Build rilis tidak punya console, jadi error ditampilkan lewat kotak pesan.
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("DANload")
            .set_description(format!("DANload tidak bisa dibuka.\n\n{error}"))
            .show();
        std::process::exit(1);
    }
}

fn run() -> Result<(), slint::PlatformError> {
    // Paksa memakai software renderer (menggambar dengan CPU, tanpa GPU),
    // supaya RAM tetap kecil. Lihat CLAUDE.md Bagian 2.6.
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;

    let window = AppWindow::new()?;
    app::setup(&window);

    // Menampilkan jendela dan menjalankan event loop sampai jendela ditutup.
    let result = window.run();
    // Pastikan tidak ada yt-dlp/FFmpeg yang tertinggal berjalan setelah jendela tertutup.
    app::shutdown();
    result
}
