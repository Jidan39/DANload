# Nama Aplikasi, Desain UI, dan Fitur Pustaka

- **Tanggal:** 2026-11-13
- **Jenis:** Dokumentasi
- **Status:** Selesai

## Latar Belakang
Pemilik projek menetapkan nama aplikasi, menyetujui yt-dlp + FFmpeg sebagai mesin unduh, memberikan dua gambar acuan desain UI, dan meminta fitur untuk menampilkan, menghapus, dan me-rename file hasil unduhan.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| CLAUDE.md | Diubah | Nama DANload, keputusan yt-dlp + FFmpeg, Bagian 2.6 (Desain UI), Bagian 2.7 (Pustaka), struktur folder, roadmap |
| docs/referensi-ui/referensi-ui-1.png | Dibuat | Gambar acuan desain 1 |
| docs/referensi-ui/referensi-ui-2.png | Dibuat | Gambar acuan desain 2 |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan
- Nama aplikasi: **DANload**; path config `%APPDATA%\DANload\`.
- yt-dlp + FFmpeg berstatus "Ditetapkan"; Rust hanya untuk GUI dan pemrosesan.
- Spesifikasi UI: tema gelap, sidebar ikon kiri, bar atas dengan pencarian & indikator platform, daftar dengan thumbnail, panel kanan, status bar bawah.
- Fitur Pustaka masuk MVP: daftar, cari/filter, buka, rename, hapus ke Recycle Bin, deteksi file hilang.
- Roadmap: Pustaka jadi Fase 6; total 9 fase (0–8).

## Dependensi
Tidak ada (belum ada kode).

## Cara Menguji
Tidak perlu diuji; cukup dibaca.

## Catatan / Yang Perlu Dicek Ulang
- Library GUI dan penyimpanan data pustaka masih menunggu keputusan (Fase 0).
- Nama folder projek masih `Media Downloader Rust`; rencana ganti ke `DANload` menunggu izin.
