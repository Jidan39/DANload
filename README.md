# DANload

Aplikasi Windows untuk mengunduh video dan audio dari YouTube, Facebook, dan Instagram.
Ditulis dengan Rust + Slint; mesin unduhnya yt-dlp dan FFmpeg.

Status: **Fase 1** (kerangka tampilan). Fitur unduh belum aktif.
Aturan kerja dan rencana lengkap ada di [CLAUDE.md](CLAUDE.md); riwayat perubahan di [CHANGELOG.md](CHANGELOG.md).

## Kebutuhan

- Windows 10/11 64-bit
- Rust stable (toolchain `x86_64-pc-windows-msvc`), dipasang lewat [rustup](https://rustup.rs)
- Visual Studio / Build Tools dengan workload "Desktop development with C++"

## Menjalankan (mode pengembangan)

```
cargo run
```

Build pertama memakan waktu beberapa menit karena semua library dikompilasi dari awal.
Build berikutnya jauh lebih cepat.

## Build rilis

```
cargo build --release
```

Hasilnya ada di `target\release\danload.exe`.

## Mengukur RAM dan CPU (target: RAM < 50 MB, CPU < 5% saat idle)

1. Jalankan `target\release\danload.exe` (versi rilis, bukan `cargo run`).
2. Diamkan jendelanya sekitar 30 detik.
3. Buka Task Manager → tab **Details** → cari `danload.exe`.
4. Catat kolom **Memory (private working set)** dan **CPU**.

## Struktur singkat

| Lokasi | Isi |
|--------|-----|
| `src/main.rs` | Titik masuk aplikasi |
| `ui/app-window.slint` | Jendela utama |
| `ui/components.slint` | Komponen kecil (tombol, chip, kolom isian) |
| `ui/theme.slint` | Warna, ukuran, dan bentuk ikon |
| `build.rs` | Mengompilasi `.slint` dan menempelkan ikon ke `.exe` |
| `assets/` | Ikon aplikasi |
| `changes/` | Catatan tiap perubahan |
