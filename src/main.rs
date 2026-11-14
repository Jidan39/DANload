//! DANload - titik masuk aplikasi.
//!
//! Fase 1: hanya membuka jendela dengan kerangka tampilan.
//! Logika unduh, pustaka, dan pengaturan ditambahkan di fase berikutnya.

// Di build rilis, jangan munculkan jendela console hitam di belakang aplikasi.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Memasukkan kode Rust hasil kompilasi file `.slint` (dibuat oleh build.rs).
// Dari sinilah tipe `AppWindow` berasal.
slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    // Paksa memakai software renderer (menggambar dengan CPU, tanpa GPU),
    // supaya RAM tetap kecil. Lihat CLAUDE.md Bagian 2.6.
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;

    let app = AppWindow::new()?;

    // Nomor versi diambil dari Cargo.toml, jadi cukup diubah di satu tempat.
    app.set_app_version(env!("CARGO_PKG_VERSION").into());

    // Menampilkan jendela dan menjalankan event loop sampai jendela ditutup.
    app.run()
}
