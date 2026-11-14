# Fase 1: Setup Cargo + Slint dan Kerangka UI

- **Tanggal:** 2026-11-14
- **Jenis:** Fitur baru
- **Status:** Sebagian — kode selesai, menunggu build dan hasil ukur RAM/CPU di Windows

## Latar Belakang
Memulai Fase 1 roadmap: projek Cargo, jendela bertema gelap dengan kerangka tata letak (CLAUDE.md Bagian 2.7), ikon, dan persiapan pengukuran performa (Bagian 2.6).

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| Cargo.toml | Dibuat | Paket `danload` 0.1.0, edition 2024, profil dev dan release |
| Cargo.lock | Dibuat | Mengunci versi semua crate (sama dengan yang sudah diuji) |
| build.rs | Dibuat | Kompilasi `.slint` (gaya `fluent-dark`) + tempel ikon ke `.exe` |
| src/main.rs | Dibuat | Memilih backend winit + software renderer, membuka jendela, mengisi nomor versi |
| ui/theme.slint | Dibuat | Warna, ukuran, dan bentuk ikon garis |
| ui/components.slint | Dibuat | Icon, Panel, NavButton, Chip, PlatformBadge, InputField, PrimaryButton, GhostButton, Segmented, FieldLabel, ValueRow |
| ui/app-window.slint | Dibuat | Jendela utama: bar atas, sidebar, area tengah, panel "Unduh baru", status bar |
| assets/icon.ico | Dibuat | Disalin dari `Youtube Downloader\icon.ico` (file asli tidak diubah) |
| assets/icon.png | Dibuat | Versi PNG 256×256 dari ikon yang sama, untuk ikon jendela |
| .gitignore | Dibuat | Mengabaikan `target/` dan `bin/` |
| README.md | Dibuat | Cara menjalankan, build rilis, dan mengukur RAM/CPU |
| CLAUDE.md | Diubah | Status Fase 1, struktur folder, versi & fitur Slint, catatan lisensi Slint |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan
- Yang sudah berfungsi: pindah halaman lewat sidebar (Unduh, Pustaka, Antrean, Pengaturan), chip filter (hanya berganti tampilan), kolom cari dan kolom URL (bisa diketik, teks panjang bergeser mengikuti kursor), tombol **Tempel**, pilihan Video/Audio (label format dan kualitas ikut berganti).
- Yang masih placeholder: tombol **Unduh** (nonaktif), pilihan format/kualitas/folder (hanya teks), penanda platform (belum menyala), kolom cari (belum mencari apa pun), daftar (selalu kosong).
- Tidak ada animasi atau timer, supaya tidak ada gambar ulang saat idle.
- Bingkai jendela masih bawaan Windows (custom title bar bersifat opsional, belum dikerjakan).
- Git belum diinisialisasi (belum ada persetujuan eksplisit).

## Dependensi
- `slint` 1.18 (fitur: `std`, `compat-1-2`, `backend-winit`, `renderer-software`, `software-renderer-path`; fitur default dimatikan)
- `slint-build` 1.18 (build)
- `winresource` 0.1 (build)

## Cara Menguji
1. `cargo run` di folder projek → jendela DANload muncul.
2. Klik tiap tombol sidebar, chip filter, Video/Audio; ketik di kolom URL; salin sebuah teks lalu klik **Tempel**.
3. `cargo build --release`, jalankan `target\release\danload.exe`, ukur di Task Manager (lihat README).

Yang sudah diuji (di Linux, bukan Windows): `cargo fmt --check`, `cargo clippy -- -D warnings`, build debug dan release berhasil; tampilan dirender dan dicek lewat tangkapan layar; klik sidebar, Video/Audio, mengetik teks panjang di kolom URL.
Yang belum diuji: build di Windows, penempelan ikon ke `.exe`, ikon jendela di taskbar, tombol **Tempel**.

## Hasil Ukur Performa
- Windows: **belum diukur** (menunggu pemilik projek).
- Indikasi di Linux (build release, jendela 1120×700): RAM ±21 MB, CPU 0% saat idle. Angka ini bukan pengganti pengukuran di Windows.

## Catatan / Yang Perlu Dicek Ulang
- Jika build di Windows gagal, kirim pesan error-nya.
- Jika ikon `.exe` tidak muncul, cek peringatan "Ikon tidak bisa ditempel" di keluaran build.
- Error saat aplikasi gagal start belum ditampilkan ke pengguna (di build rilis tidak ada console); akan ditangani di Fase 2.
- Syarat lisensi Slint perlu dicek sebelum aplikasi dibagikan.
