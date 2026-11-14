# Ganti Nama Folder, Inno Setup, dan Lingkungan Pengembangan

- **Tanggal:** 2026-11-14
- **Jenis:** Dokumentasi / Struktur folder
- **Status:** Selesai

## Latar Belakang
Pemilik projek mengizinkan folder projek diganti namanya menjadi `DANload` dan memilih Inno Setup sebagai pembuat installer. Sekalian, `CLAUDE.md` ditinjau ulang supaya siap dipakai untuk mulai membuat aplikasi.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| `D:\Code Project\Media Downloader Rust\` → `D:\Code Project\DANload\` | Diganti nama | Folder projek |
| CLAUDE.md | Diubah | Installer = Inno Setup (ditetapkan); Bagian 4 baru "Lingkungan Pengembangan"; aturan kode khusus Windows & Slint; tujuan 2.2 dan catatan struktur folder diperbarui; penomoran bagian 4–11 |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan
- Fase 0 kini lengkap (termasuk Inno Setup).
- Bagian 4 menjelaskan toolchain yang dibutuhkan (Rust stable MSVC, VS Build Tools, Inno Setup 6) dan bahwa build/pengukuran dilakukan di Windows pemilik projek.
- Aturan kode tambahan: tanpa jendela console di build release, `CREATE_NO_WINDOW` untuk yt-dlp/FFmpeg, logika tidak ditaruh di `.slint`, update UI dari thread lewat mekanisme Slint, path memakai `PathBuf`.

## Dependensi
Tidak ada.

## Cara Menguji
Buka `D:\Code Project\DANload\` dan pastikan semua file ada.

## Hasil Ukur Performa
Tidak relevan.

## Catatan / Yang Perlu Dicek Ulang
- Pastikan Rust sudah terpasang di Windows sebelum Fase 1.
