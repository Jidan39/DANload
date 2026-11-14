# Fase 2A: Config, Cek Komponen, dan Halaman Pengaturan

- **Tanggal:** 2026-11-14
- **Jenis:** Fitur baru
- **Status:** Sebagian — kode selesai dan lolos uji di Linux, menunggu uji di Windows

## Latar Belakang
Fase 2 dipecah menjadi 2A dan 2B (disetujui pemilik projek). Bagian 2A menyiapkan fondasi yang dibutuhkan wizard setup dan fitur unduh: penyimpanan pengaturan, pencarian komponen eksternal, dan halaman Pengaturan.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| src/config.rs | Dibuat | `Config` (folder unduhan, mode, penanda setup), baca/tulis `config.json`, folder unduhan bawaan + 6 unit test |
| src/deps.rs | Dibuat | Mencari yt-dlp/FFmpeg/ffprobe di 3 lokasi, membaca versi, batas waktu 30 detik, tanpa jendela console + 8 unit test |
| src/app.rs | Dibuat | Mengisi data awal ke UI, menangani aksi pengguna, menjalankan cek komponen di thread terpisah |
| src/main.rs | Diubah | Mendaftarkan modul baru; error saat start ditampilkan lewat kotak pesan |
| ui/settings-page.slint | Dibuat | Halaman Pengaturan: daftar komponen + status, tombol "Cek ulang", folder unduhan + "Ubah..." |
| ui/app-window.slint | Diubah | Properti/callback baru, halaman Pengaturan, status bar dinamis, "Simpan ke" menampilkan folder dan bisa diklik |
| ui/components.slint | Diubah | `NavButton.alert`, `GhostButton.enabled`, `Segmented.selected`, `ValueRow.clickable` |
| ui/theme.slint | Diubah | Warna `warning` |
| Cargo.toml, Cargo.lock | Diubah | Dependensi baru (lihat bawah) |
| README.md | Diubah | Status, cara menjalankan test, struktur |
| CLAUDE.md | Diubah | Roadmap 2A/2B, tabel teknologi, sumber FFmpeg, struktur folder, catatan git, pertanyaan terbuka soal runtime JavaScript |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan
- **Config** disimpan di `%APPDATA%\DANload\config.json`, ditulis lewat file sementara lalu di-rename. File yang hilang atau rusak tidak membuat aplikasi gagal: dipakai nilai bawaan.
- **Folder unduhan bawaan:** `%USERPROFILE%\Downloads\DANload` (belum dibuat di disk sampai ada unduhan).
- **Pencarian komponen** sesuai urutan di CLAUDE.md Bagian 2.5: `%LOCALAPPDATA%\DANload\bin\` → `bin\` di samping exe → `PATH`.
- **Status komponen:** Memeriksa / Terpasang (versi + lokasi) / Bermasalah (file ada tapi gagal dijalankan) / Belum ada.
- Cek komponen berjalan di 3 thread pendek (satu per komponen) dan hasilnya dikirim ke UI lewat `upgrade_in_event_loop`; UI tidak diblok.
- Mode Video/Audio dan folder unduhan langsung tersimpan saat diubah. Jika gagal menyimpan, pesannya muncul di status bar.
- Titik kuning di tombol Pengaturan dan teks status bar menandai komponen yang belum lengkap.
- Halaman Pengaturan bisa digulir jika jendela pendek.

## Dependensi
- `serde` 1 (fitur `derive`) dan `serde_json` 1 — config
- `rfd` 0.18 — dialog pilih folder dan kotak pesan error (fitur `xdg-portal` hanya berpengaruh di Linux)
- `slint`: tambah fitur `raw-window-handle-06` (agar dialog menempel ke jendela DANload)

## Cara Menguji
1. `cargo test` → 14 test (di Windows 12, karena 2 test khusus Linux) harus lolos.
2. `cargo run`, buka **Pengaturan**: ketiga komponen menampilkan status. Yang terpasang lewat winget seharusnya "Terpasang" dengan versi dan lokasinya.
3. Klik **Cek ulang**.
4. Klik **Ubah...** (atau baris "Simpan ke" di panel kanan), pilih folder → teks folder berubah di kedua tempat.
5. Ganti Video/Audio, tutup aplikasi, buka lagi → pilihan mode dan folder masih sama.
6. Lihat isi `%APPDATA%\DANload\config.json`.

Sudah diuji (Linux): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, 14 unit test, build debug dan release; halaman Pengaturan dirender dan dicek lewat tangkapan layar dengan komponen palsu (terpasang, bermasalah, lalu diperbaiki + "Cek ulang"); penyimpanan mode ke config.json; gulir pada jendela 920×580.
Belum diuji: semua hal di Windows — dialog pilih folder, kotak pesan error, `CREATE_NO_WINDOW`, pembacaan versi yt-dlp.exe/ffmpeg.exe asli.

## Hasil Ukur Performa
- Windows: **belum diukur**.
- Indikasi di Linux (build release, 1120×700, halaman Pengaturan terbuka): RAM ±22 MB, CPU 0% saat idle, thread pekerja sudah selesai.

## Catatan / Yang Perlu Dicek Ulang
- Angka RAM/CPU versi rilis Fase 1 di Windows masih belum dilaporkan.
- Config yang rusak ditimpa diam-diam pada penyimpanan berikutnya (tanpa cadangan).
- Folder unduhan yang dipilih belum dicek apakah bisa ditulisi; pengecekan dilakukan saat fitur unduh dibuat (Fase 3).
