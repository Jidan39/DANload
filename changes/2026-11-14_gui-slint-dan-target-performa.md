# GUI Slint dan Target Performa

- **Tanggal:** 2026-11-14
- **Jenis:** Dokumentasi
- **Status:** Selesai

## Latar Belakang
Pemilik projek ingin DANload ringan: RAM < 50 MB dan CPU < 5% saat idle. Tauri tidak memenuhi target karena memakai WebView2 (berbasis Chromium); egui berisiko karena memuat stack GPU. Diputuskan memakai Slint dengan software renderer.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| CLAUDE.md | Diubah | GUI = Slint (ditetapkan), Bagian 2.6 Target Performa (baru), penomoran Desain UI → 2.7 & Pustaka → 2.8, roadmap Fase 0–1, Definisi Selesai, template catatan, struktur folder (`ui/`) |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan
- Target: RAM proses DANload < 50 MB dan CPU < 5% saat idle, build release, diukur di Task Manager; yt-dlp/FFmpeg tidak dihitung.
- Aturan penjaga performa: tanpa redraw terus-menerus, progress dibatasi frekuensinya, thumbnail kecil + cache disk, FFmpeg prioritas rendah.
- Fase 1 kini wajib mengukur RAM/CPU jendela kosong sebelum lanjut.
- Template catatan perubahan mendapat bagian "Hasil Ukur Performa".

## Dependensi
Tidak ada (belum ada kode).

## Cara Menguji
Tidak perlu diuji; cukup dibaca.

## Hasil Ukur Performa
Tidak relevan.

## Catatan / Yang Perlu Dicek Ulang
- Angka target belum terbukti; verifikasi pertama dilakukan di Fase 1.
- Ganti nama folder ke `DANload` masih menunggu izin.
