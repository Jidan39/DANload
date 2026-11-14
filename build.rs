//! Skrip build: dijalankan Cargo sebelum mengompilasi `src/`.
//!
//! Tugasnya dua:
//! 1. Mengubah file `.slint` (tampilan) menjadi kode Rust.
//! 2. Menempelkan ikon ke `DANload.exe` (khusus Windows).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Gaya "fluent-dark" dipakai supaya widget bawaan Slint (jika nanti dipakai)
    // otomatis bertema gelap, sesuai CLAUDE.md Bagian 2.7.
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app-window.slint", config)?;

    // CARGO_CFG_TARGET_OS = sistem operasi tujuan build (bukan komputer yang mem-build).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        resource.set("ProductName", "DANload");
        resource.set("FileDescription", "DANload");
        // Kalau ikon gagal ditempel (mis. rc.exe tidak ditemukan), build tetap lanjut
        // dengan peringatan: aplikasi tetap jalan, hanya ikon .exe yang kosong.
        if let Err(error) = resource.compile() {
            println!("cargo:warning=Ikon tidak bisa ditempel ke .exe: {error}");
        }
    }

    Ok(())
}
