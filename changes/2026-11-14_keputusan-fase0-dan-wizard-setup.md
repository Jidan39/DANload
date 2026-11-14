# Keputusan Fase 0 dan Rancangan Instalasi & Wizard Setup

- **Tanggal:** 2026-11-14
- **Jenis:** Dokumentasi
- **Status:** Selesai

## Latar Belakang
Pemilik projek menyetujui semua rekomendasi Fase 0 dan meminta agar pemasangan aplikasi mudah bagi pengguna: ada UI di dalam aplikasi untuk memasang yt-dlp, FFmpeg, dan kebutuhan lain, serta untuk mengatur folder unduhan.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| CLAUDE.md | Diubah | Tabel Teknologi (keputusan Fase 0), fitur MVP (wizard + installer), Bagian 2.9 baru, roadmap Fase 0/2/8, struktur folder (`setup.rs`, `installer/`) |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan
- Ditetapkan: `std::thread` + channel (tanpa tokio), data pustaka dalam JSON.
- **Penyesuaian** lokasi yt-dlp/FFmpeg: utama di `%LOCALAPPDATA%\DANload\bin\` (bukan hanya di samping exe), karena folder aplikasi bisa tidak boleh ditulisi tanpa hak Administrator. Urutan pencarian: `%LOCALAPPDATA%\DANload\bin\` → `bin\` di samping exe → `PATH`.
- Bagian 2.9: installer per pengguna (tanpa Administrator) + wizard 5 langkah (selamat datang, cek komponen, pasang otomatis/manual, folder unduhan, selesai), beserta aturan keamanan dan penanganan error.
- Fase 0 ditandai selesai (kecuali pilihan alat installer). Wizard masuk Fase 2, installer masuk Fase 8.

## Dependensi
Belum ada. Rencana crate untuk wizard (`ureq`, `zip`) akan diminta persetujuannya saat Fase 2.

## Cara Menguji
Tidak perlu diuji; cukup dibaca.

## Hasil Ukur Performa
Tidak relevan.

## Catatan / Yang Perlu Dicek Ulang
- Alat pembuat installer belum diputuskan (rekomendasi: Inno Setup).
- Sumber unduhan FFmpeg dan ketersediaan checksum perlu dicek ulang saat Fase 2.
