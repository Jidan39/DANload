# Fase 2B: Wizard Setup Awal dan Pilihan Platform

- **Tanggal:** 2026-11-14
- **Jenis:** Fitur baru
- **Status:** Sebagian — kode selesai dan lolos uji di Linux, menunggu uji di Windows

## Latar Belakang
- Bagian 2B dari Fase 2 (CLAUDE.md Bagian 2.9): pengguna awam bisa memasang yt-dlp dan FFmpeg tanpa terminal, lewat wizard di dalam aplikasi.
- Permintaan baru pemilik projek: pengguna **memilih platform** (YouTube / Facebook / Instagram) di kanan atas sebelum bisa mengunduh, dengan **YouTube sebagai bawaan**.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| src/setup.rs | Dibuat | Daftar sumber resmi, unduh streaming ke disk + SHA-256, ekstrak zip, pasang ke `%LOCALAPPDATA%\DANload\bin\`, batal, sumber cadangan, pasang dari file pilihan pengguna + 13 unit test (1 test butuh internet, `#[ignore]`) |
| src/platform.rs | Dibuat | `Platform` (YouTube/Facebook/Instagram), pengecekan kecocokan URL dengan platform terpilih, pesan untuk UI + 9 unit test |
| src/app.rs | Diubah | Wizard (pasang di thread terpisah, progress, batal, laporan basi diabaikan), wizard otomatis saat pertama kali/komponen hilang, subfolder Video/Audio, pengecekan URL |
| src/config.rs | Diubah | Kolom baru `separate_subfolders`; `setup_completed` kini dipakai |
| src/deps.rs | Diubah | `windows_file_name()` (nama program di dalam arsip); `file_name()` tanpa `.exe` di Linux untuk pengujian |
| src/main.rs | Diubah | Mendaftarkan modul `platform` dan `setup` |
| ui/setup-wizard.slint | Dibuat | Wizard 4 langkah: Selamat datang, Komponen, Folder unduhan, Selesai |
| ui/app-window.slint | Diubah | Pilihan platform di bar atas (bawaan YouTube), label & contoh URL ikut platform, pesan kecocokan URL, lapisan wizard |
| ui/settings-page.slint | Diubah | Tombol "Pasang / perbarui...", centang subfolder Video/Audio |
| ui/components.slint | Diubah | `PlatformBadge` bisa diklik, komponen baru `Checkbox` |
| Cargo.toml, Cargo.lock | Diubah | Dependensi baru (lihat bawah) |
| README.md | Diubah | Pemakaian pertama, test butuh internet, struktur |
| CLAUDE.md | Diubah | Aturan pilihan platform (2.3, 2.7), sumber yt-dlp/FFmpeg (2.5), wizard 4 langkah (2.9), cara uji (4), struktur folder, roadmap |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan

**Wizard setup**
- Muncul otomatis saat setup belum pernah selesai. Jika setup sudah selesai tetapi ada komponen yang hilang/rusak, wizard terbuka otomatis di langkah Komponen.
- **Pasang otomatis** hanya memasang paket yang belum siap; **Pasang versi terbaru** memasang ulang semuanya.
- Sumber: yt-dlp dari GitHub resmi; FFmpeg dari gyan.dev, dengan cadangan BtbN (GitHub). Checksum SHA-256 wajib cocok.
- File diunduh langsung ke disk ke folder sementara `.tmp-N` di folder komponen; baru dipindah setelah checksum cocok dan program diekstrak. Folder sementara selalu dihapus, baik berhasil, gagal, maupun dibatalkan.
- Dari arsip FFmpeg hanya `ffmpeg.exe` dan `ffprobe.exe` yang dipasang (ffplay dan file lain tidak ikut).
- Progress dikirim ke UI paling sering 10 kali per detik.
- Pesan error dalam Bahasa Indonesia: tidak ada internet, file tidak ada di server (404), checksum tidak cocok, unduhan terputus/tidak lengkap, disk penuh, akses ditolak/antivirus.
- **Nanti saja** menutup wizard tanpa menandai setup selesai.

**Pilihan platform**
- Tombol YouTube / Facebook / Instagram di kanan atas bisa diklik; **YouTube terpilih setiap aplikasi dibuka**.
- Label ("URL YOUTUBE" dst.) dan contoh URL di panel kanan mengikuti platform terpilih.
- Di bawah kolom URL muncul pesan: hijau jika link cocok, kuning jika link dari platform lain, bukan dari platform yang didukung, atau bukan link.
- Pengecekan berdasarkan nama host, termasuk subdomain (mis. `m.youtube.com`, `web.facebook.com`) dan link pendek (`youtu.be`, `fb.watch`, `instagr.am`). Domain tiruan seperti `notyoutube.com` atau `youtube.com@evil.example` tidak dianggap cocok.
- Hasil pengecekan (`valid`) sudah tersedia untuk mengaktifkan tombol Unduh di Fase 3.

## Dependensi
- `ureq` 3.5 (fitur `rustls`, `platform-verifier`; tanpa fitur default) — unduhan HTTPS memakai sertifikat Windows
- `sha2` 0.11 — checksum SHA-256
- `zip` 9.5 (fitur `deflate-flate2-zlib-rs` saja; tanpa kompresor zopfli yang tidak dibutuhkan) — ekstrak arsip FFmpeg

## Cara Menguji
1. `cargo test` → 38 test lolos di Linux; di Windows seharusnya 35 (3 test khusus Linux dilewati).
2. `cargo test -- --ignored` (butuh internet) → checksum dari ketiga sumber resmi bisa diambil.
3. Untuk mengulang wizard dari awal, hapus atau ganti nama `%APPDATA%\DANload\config.json`, lalu `cargo run`.
4. Di wizard: **Mulai** → **Pasang otomatis** → tunggu progress → **Lanjut** → pilih folder → **Lanjut** → **Mulai pakai DANload**.
5. Coba juga **Batal** di tengah unduhan, dan **Pilih ffmpeg.exe...**.
6. Di jendela utama: tempel link YouTube saat Facebook terpilih (pesan kuning), lalu klik YouTube di kanan atas (pesan hijau).

Sudah diuji (Linux): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, 38 unit test; seluruh alur wizard lewat tangkapan layar dengan server lokal (pasang, progress, selesai, batal, gagal karena server mati, wizard otomatis saat komponen hilang); pilihan platform dan pesan URL; centang subfolder tersimpan di config. Unduhan **yt-dlp.exe asli** dari GitHub (23,5 MB) lewat jalur yang sama dengan aplikasi: redirect diikuti, checksum cocok, folder sementara bersih. File checksum asli BtbN dan alamat `.sha256` gyan.dev dicek tersedia dan formatnya sesuai.

Belum diuji: semua hal di Windows (build dengan crate baru, unduhan FFmpeg asli dari gyan.dev, dialog pilih file, sertifikat Windows), dan unduhan FFmpeg asli dari workspace (gyan.dev diblokir jaringan workspace).

## Hasil Ukur Performa
- Windows: **belum diukur**.
- Indikasi di Linux (build release, 1120×700): RAM ±22 MB di jendela utama dan ±23 MB saat wizard terbuka; CPU 0% saat idle. Ukuran file program ±16 MB.

## Catatan / Yang Perlu Dicek Ulang
- Angka RAM/CPU versi rilis di Windows untuk Fase 1, 2A, dan 2B masih belum dilaporkan.
- Jika koneksi macet total di tengah unduhan, tombol **Batal** langsung mengembalikan tampilan, tetapi thread unduhan baru berhenti setelah koneksi bergerak lagi atau batas waktu tercapai.
- Pilihan platform tidak disimpan di config (selalu kembali ke YouTube saat aplikasi dibuka), sesuai permintaan "bawaan YouTube".
- Pertanyaan terbuka soal runtime JavaScript untuk yt-dlp tetap diputuskan di Fase 3.
