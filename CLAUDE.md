# CLAUDE.md — DANload

File ini berisi aturan kerja dan konteks projek. **Baca seluruh file ini sebelum melakukan apa pun di projek ini.**
Jika ada aturan di sini yang bertentangan dengan instruksi langsung dari pemilik projek (Jidan) di percakapan, ikuti instruksi terbaru, lalu usulkan untuk memperbarui file ini.

---

## 1. Aturan Utama: Konfirmasi Dulu, Baru Kerjakan

1. **Jangan langsung mengubah kode.** Untuk setiap permintaan, jelaskan dulu secara singkat:
   - apa yang dipahami dari permintaan,
   - rencana langkah yang akan dilakukan,
   - file yang akan **dibuat / diubah / dihapus**,
   - dependensi (crate) baru yang akan ditambahkan, jika ada.
2. **Tunggu persetujuan** ("oke", "lanjut", "gas", dsb.) sebelum mengeksekusi.
3. Pengecualian: perubahan sangat kecil yang sudah diminta secara eksplisit (mis. "ganti teks tombol jadi X") boleh langsung dikerjakan, tetapi tetap dilaporkan dan tetap dibuatkan catatan perubahan (lihat Bagian 3).
4. **Wajib minta izin terpisah** sebelum:
   - menghapus, memindahkan, atau menimpa file;
   - menambah atau menghapus dependensi di `Cargo.toml`;
   - mengubah struktur folder;
   - menjalankan `git commit`, `git push`, atau membuat release;
   - mengunduh/menyertakan binary pihak ketiga (yt-dlp, FFmpeg) ke dalam repo.
5. Jika permintaan ambigu dan salah tebak akan membuang waktu, **tanya dulu** (cukup pertanyaan yang paling penting). Jika risikonya kecil, pakai asumsi yang masuk akal dan sebutkan asumsinya.
6. Jika ada keputusan teknis yang berpengaruh besar (pilihan library GUI, arsitektur, cara distribusi), berikan **2–3 opsi** beserta kelebihan, kekurangan, dan risikonya, lalu **satu rekomendasi**. Jangan sekadar mengikuti pilihan saya jika secara teknis kurang tepat — jelaskan alasannya.

---

## 2. Tentang Projek

### 2.1 Ringkasan
**Nama aplikasi: DANload**

Aplikasi **desktop Windows** untuk mengunduh **video dan audio** dari **YouTube, Facebook, dan Instagram**, dengan pilihan berbagai format output. Ditulis dalam **Rust**.

Projek ini adalah penulisan ulang (rewrite) dan pengembangan dari aplikasi lama:
`D:\Code Project\Youtube Downloader` (Python + Tkinter + yt-dlp).
Folder lama itu **hanya sebagai referensi** — **jangan diubah**.

### 2.2 Tujuan
- Aplikasi GUI yang ringan, cepat dibuka, dan mudah dipakai orang awam.
- Dibagikan sebagai **installer Inno Setup** (`DANload-Setup.exe`) yang mudah dipakai orang awam.
- Mendukung lebih banyak situs dan format dibanding aplikasi lama.
- Kode rapi, mudah dibaca, dan jadi bahan belajar Rust.

### 2.3 Fitur yang Diharapkan

**Wajib (MVP) — minimal setara aplikasi lama:**
- [ ] Input URL + tombol **Tempel** dari clipboard
- [ ] Deteksi otomatis sumber: YouTube / Facebook / Instagram
- [ ] Pilih mode: **Video** atau **Audio**
- [ ] Pilih format output (lihat 2.4)
- [ ] Pilih resolusi maksimum (video) / bitrate (audio)
- [ ] Pilih folder simpan, dan folder terakhir diingat
- [ ] Opsi unduh seluruh playlist
- [ ] Progress bar (persen, kecepatan, ETA) + panel log
- [ ] Pengecekan yt-dlp dan FFmpeg saat aplikasi dibuka, dengan pesan error yang jelas
- [ ] **Wizard setup awal** di dalam aplikasi: memasang yt-dlp & FFmpeg otomatis dan mengatur folder unduhan (detail di Bagian 2.9)
- [ ] **Installer Windows** yang mudah dipakai orang awam (detail di Bagian 2.9)
- [ ] Pengaturan disimpan di `%APPDATA%\DANload\config.json`
- [ ] **Pustaka (Library):** menampilkan semua file yang sudah diunduh, bisa **dihapus** dan **di-rename** dari dalam aplikasi (detail di Bagian 2.8)
- [ ] UI tidak macet (freeze) selama proses unduh
- [ ] Ikon aplikasi

**Tambahan (setelah MVP stabil):**
- [ ] Antrean (queue) beberapa URL sekaligus
- [ ] Tombol batal (cancel) unduhan
- [ ] Edit singkat lain di pustaka (mis. potong/trim audio-video via FFmpeg) — dipertimbangkan nanti
- [ ] Pratinjau info video sebelum unduh (judul, durasi, thumbnail, daftar kualitas)
- [ ] Unduh subtitle
- [ ] Tombol update yt-dlp & FFmpeg dari dalam aplikasi (memakai mekanisme yang sama dengan wizard)
- [ ] Dukungan cookies browser (untuk konten yang butuh login, mis. sebagian Instagram)
- [ ] Opsi tema terang (default: gelap)

### 2.4 Format yang Didukung

| Mode  | Format | Catatan |
|-------|--------|---------|
| Video | MP4 | Default, paling kompatibel |
| Video | MKV | Tanpa re-encode, hanya merge |
| Video | WEBM | Format asli YouTube untuk banyak resolusi |
| Video | MOV | Remux/re-encode via FFmpeg |
| Video | AVI | Perlu re-encode, ukuran besar |
| Video | MPEG (`.mpg`) | Perlu re-encode (MPEG-2), proses lebih lama |
| Audio | MP3 | Default |
| Audio | M4A (AAC) | |
| Audio | OPUS | |
| Audio | OGG (Vorbis) | |
| Audio | FLAC | Lossless (ukuran besar, tapi kualitas tetap terbatas oleh sumber) |
| Audio | WAV | Tanpa kompresi |

Catatan: format yang butuh **re-encode** (AVI, MPEG, kadang MOV) harus diberi peringatan di UI bahwa prosesnya lebih lama.

### 2.5 Teknologi

| Bagian | Pilihan | Status |
|--------|---------|--------|
| Bahasa | Rust (edition terbaru yang stabil) | ✅ Ditetapkan |
| Target OS | Windows 10/11 (64-bit) | ✅ Ditetapkan |
| Mesin unduh | **yt-dlp** (dipanggil sebagai proses eksternal) | ✅ Ditetapkan |
| Konversi/merge | **FFmpeg** (proses eksternal) | ✅ Ditetapkan |
| GUI | **Slint 1.18** dengan **software renderer** (tanpa GPU); fitur: `std`, `compat-1-2`, `backend-winit`, `renderer-software`, `software-renderer-path`. Cadangan: `egui` jika Slint gagal memenuhi target | ✅ Ditetapkan |
| Ikon .exe | `winresource` (hanya saat build) | ✅ Ditetapkan |
| Async/proses | `std::thread` + channel (**tanpa tokio**) | ✅ Ditetapkan |
| Config | `serde` + `serde_json` | ✅ Ditetapkan |
| Dialog bawaan Windows (pilih folder, kotak pesan) | `rfd` | ✅ Ditetapkan |
| Data pustaka/riwayat | File **JSON** (`serde_json`) di `%APPDATA%\DANload\` | ✅ Ditetapkan |
| Lokasi yt-dlp/FFmpeg | `%LOCALAPPDATA%\DANload\bin\` → folder `bin\` di samping `DANload.exe` → `PATH` (urutan pencarian) | ✅ Ditetapkan |
| Unduh komponen (wizard) | `ureq` (HTTP, blocking) + `sha2` (checksum) + `zip` (ekstrak) | ✅ Disetujui, ditambahkan di Fase 2B |
| Sumber FFmpeg | Utama: **gyan.dev essentials `.zip`** (±88 MB, ada `.sha256`). Cadangan: **BtbN** GPL static `.zip` (±163 MiB). Keduanya ditautkan dari ffmpeg.org | ✅ Ditetapkan |
| Installer | **Inno Setup** | ✅ Ditetapkan |
| Hapus file | Pindah ke **Recycle Bin** (mis. crate `trash`), bukan hapus permanen | ✅ Disarankan |
| Error handling | `anyhow` / `thiserror` | ✅ Disarankan |

**Keputusan (disetujui pemilik projek, 2026-11-13):** mesin unduh memakai **yt-dlp + FFmpeg**; Rust hanya untuk **GUI dan pemrosesan** (pengatur proses, pembaca progress, pustaka, konfigurasi). Alasannya: YouTube, Facebook, dan Instagram sering mengubah sistemnya, dan yt-dlp dirawat aktif oleh komunitas, sehingga aplikasi cukup memperbarui yt-dlp tanpa perawatan kode ekstraktor sendiri. **Jangan** menulis ekstraktor situs sendiri di Rust.

### 2.6 Target Performa (Wajib)

Disetujui pemilik projek, 2026-11-14.

| Kondisi | Target |
|---------|--------|
| RAM proses `DANload.exe` saat **idle** (jendela ukuran normal, tidak sedang mengunduh) | **< 50 MB** |
| CPU proses `DANload.exe` saat **idle** | **< 5%** |

- Yang diukur hanya proses DANload. Proses **yt-dlp** dan **FFmpeg** tidak dihitung, karena berjalan terpisah dan hanya aktif saat mengunduh/konversi.
- Pengukuran memakai Task Manager (kolom *Memory* / *Private working set*), build **release** (`cargo build --release`).
- Kenapa Slint + software renderer: tidak memuat stack GPU/driver grafis yang bisa menambah puluhan MB, dan hanya menggambar ulang bagian UI yang berubah.
- Aturan untuk menjaga target:
  - Tidak ada animasi/timer yang menggambar ulang terus-menerus saat idle.
  - Progress unduhan dikirim ke UI dengan batas frekuensi (mis. maksimal ±10 kali per detik).
  - Thumbnail dimuat dalam ukuran kecil, hanya untuk item yang terlihat, dan di-cache di disk (bukan disimpan semua di RAM).
  - FFmpeg dijalankan dengan prioritas proses rendah supaya komputer tidak terasa berat saat konversi.
  - Jangan menambah crate berat tanpa alasan jelas dan persetujuan.
- Jika target tidak tercapai, **laporkan angka hasil ukur** dan usulkan perbaikan sebelum melanjutkan fase berikutnya.

### 2.7 Desain UI

Acuan visual: `docs/referensi-ui/referensi-ui-1.png` dan `docs/referensi-ui/referensi-ui-2.png`.
Yang ditiru adalah **gaya dan tata letaknya**, bukan nama, logo, atau konten aplikasi di gambar tersebut.

**Gaya umum**
- Tema **gelap** (latar hampir hitam, panel abu-abu gelap, teks putih/abu-abu muda).
- Sudut membulat (*rounded*) pada panel, tombol, kolom input, dan *chip*.
- Satu warna aksen untuk tombol utama; hijau/merah/biru hanya untuk status (selesai, gagal, sedang mengunduh).
- Font sans-serif bersih; hierarki jelas (judul tebal, info sekunder abu-abu kecil).
- Jendela tanpa bingkai bawaan Windows (*custom title bar*) dengan tombol minimize/maximize/close sendiri — opsional, jika library GUI mendukung.

**Tata letak**
```
┌──────────────────────────────────────────────────────────────────┐
│ ● DANload        [ 🔍 Cari file unduhan...  ]   [YT][FB][IG]  _ □ x │
├────┬──────────────────────────────────────────┬───────────────────┤
│ ⬇  │ [Semua][Video][Audio][YouTube][FB][IG]   │  PANEL KANAN      │
│ 📚 │ ┌──────────────────────────────────────┐ │  - Unduh baru:    │
│ ⏳ │ │ 🖼 Judul video        MP4 · 1080p   ⋮ │ │    URL, mode,     │
│ ⚙  │ │    YouTube · 45 MB · 13 Nov 19:40    │ │    format,        │
│    │ │ 🖼 Judul audio        MP3 · 192k    ⋮ │ │    kualitas,      │
│    │ │    Instagram · 4 MB · 12 Nov 21:02   │ │    folder,        │
│    │ └──────────────────────────────────────┘ │    [ ⬇ Unduh ]    │
│    │                                          │  - atau detail    │
│    │                                          │    file terpilih  │
├────┴──────────────────────────────────────────┴───────────────────┤
│ Siap.                         ● 2 sedang mengunduh   Batal semua v0.1.0 │
└──────────────────────────────────────────────────────────────────┘
```
- **Sidebar kiri (ikon):** Unduh, Pustaka, Antrean (dengan badge jumlah), Pengaturan.
- **Bar atas:** nama aplikasi, kolom pencarian, indikator platform (YouTube / Facebook / Instagram) yang menyala otomatis sesuai URL.
- **Area tengah:** daftar item dengan thumbnail, judul, platform, format, kualitas, ukuran, tanggal; *chip* filter di atas daftar; centang untuk pilih banyak.
- **Panel kanan:** form unduh baru, atau detail file yang sedang dipilih (thumbnail besar, info, tombol aksi).
- **Status bar bawah:** status, jumlah unduhan aktif, tombol "Batal semua", versi aplikasi.

### 2.8 Fitur Pustaka (Library)

Menampilkan file yang sudah diunduh lewat DANload dan mengelolanya langsung dari aplikasi.

| Fitur | Perilaku yang diharapkan |
|-------|--------------------------|
| Daftar unduhan | Thumbnail, judul, platform, format, kualitas, ukuran, durasi, tanggal unduh, lokasi file |
| Cari & filter | Cari berdasarkan nama; filter Video/Audio dan per platform; urutkan berdasarkan tanggal/nama/ukuran |
| Buka | Buka file dengan pemutar bawaan Windows; tombol "Tampilkan di folder" (Explorer dengan file terpilih) |
| **Rename** | Ubah nama file langsung dari aplikasi. Validasi karakter terlarang Windows, tolak jika nama sudah dipakai, ekstensi tidak boleh ikut berubah, catatan pustaka ikut diperbarui |
| **Hapus** | Pilihan: (a) hapus dari daftar saja, atau (b) hapus file → dipindah ke **Recycle Bin**. Selalu ada dialog konfirmasi; mendukung hapus banyak sekaligus |
| File hilang | Jika file dipindah/dihapus di luar aplikasi, item ditandai "File tidak ditemukan" (tidak error), dengan opsi hapus dari daftar |
| Salin URL sumber | Salin link asli video ke clipboard |
| Unduh ulang | Unduh lagi dari URL sumber dengan format yang sama atau berbeda |

Data pustaka disimpan di `%APPDATA%\DANload\` (bukan di folder hasil unduhan), sehingga tetap ada walaupun folder unduhan diganti.

### 2.9 Instalasi & Wizard Setup Awal

Tujuan: orang awam bisa memasang dan memakai DANload **tanpa membuka terminal** dan tanpa memasang yt-dlp/FFmpeg secara manual.

**Tahap 1 — Installer Windows**
- Memasang `DANload.exe` **per pengguna** di `%LOCALAPPDATA%\Programs\DANload\` (tidak butuh hak Administrator).
- Membuat shortcut Start Menu dan (opsional) Desktop; menyediakan uninstaller di "Apps & features".
- Opsi "Jalankan DANload" di akhir instalasi.
- Installer **tidak** menyertakan yt-dlp/FFmpeg; keduanya dipasang oleh wizard di dalam aplikasi (installer tetap kecil dan komponen selalu versi terbaru).
- Uninstall bertanya apakah data pengguna (`%APPDATA%\DANload\`, komponen di `%LOCALAPPDATA%\DANload\bin\`) ikut dihapus. File hasil unduhan **tidak pernah** dihapus oleh uninstaller.

**Tahap 2 — Wizard setup di dalam aplikasi** (muncul otomatis saat pertama kali dibuka, atau jika komponen hilang/rusak)

| Langkah | Isi |
|---------|-----|
| 1. Selamat datang | Penjelasan singkat apa yang akan dipasang dan kenapa dibutuhkan |
| 2. Cek komponen | Tampilkan status yt-dlp dan FFmpeg: ✅ ditemukan (lokasi + versi) / ❌ belum ada |
| 3. Pasang komponen | Tombol **Pasang otomatis**: unduh dari sumber resmi, tampilkan progress & ukuran, verifikasi, ekstrak, pasang ke `%LOCALAPPDATA%\DANload\bin\`. Alternatif: **Pilih file manual** (jika pengguna sudah punya yt-dlp.exe/ffmpeg.exe) atau **Buka halaman unduhan** |
| 4. Folder unduhan | Pilih folder default (awal: `%USERPROFILE%\Downloads\DANload`); opsi subfolder otomatis `Video\` dan `Audio\` |
| 5. Selesai | Ringkasan pengaturan → masuk ke tampilan utama |

Aturan wizard:
- Bisa dibuka lagi kapan saja dari **Pengaturan** (pasang ulang, update komponen, ganti folder unduhan).
- Aplikasi tetap bisa dibuka walau komponen belum ada, tetapi tombol Unduh dinonaktifkan dengan pesan jelas + tombol menuju wizard.
- Sumber unduhan hanya dari sumber resmi: yt-dlp dari rilis GitHub resmi; FFmpeg dari build Windows yang ditautkan di situs resmi ffmpeg.org. URL ditulis di satu tempat di kode supaya mudah diganti.
- Verifikasi checksum (SHA-256) jika sumber menyediakannya; jika gagal verifikasi, file dibuang dan pengguna diberi tahu.
- File diunduh **langsung ke disk secara streaming** (bukan ditampung di RAM), ke folder sementara dulu, lalu dipindah setelah lengkap & terverifikasi — supaya tidak ada file setengah jadi.
- Tangani kondisi: tidak ada internet, unduhan terputus (bisa coba lagi), ruang disk tidak cukup, antivirus memblokir file.
- Ganti folder unduhan **tidak** memindahkan file lama; Pustaka tetap menyimpan lokasi asli setiap file.
- Lisensi: FFmpeg dan yt-dlp diunduh dari sumber resminya oleh pengguna sendiri (tidak dibundel di installer). Cantumkan nama & lisensi keduanya di halaman "Tentang".
- **Pertanyaan terbuka (putuskan di Fase 3):** dokumentasi yt-dlp sangat menganjurkan *runtime* JavaScript (kemungkinan Deno) di samping ffmpeg/ffprobe, terutama untuk YouTube. Wizard dibuat berbasis daftar komponen supaya komponen ketiga bisa ditambah dengan mudah. *(Belum diverifikasi dengan unduhan nyata.)*

---

## 3. Aturan Catatan Perubahan (Wajib)

**Setiap kali ada perubahan pada projek** (kode, konfigurasi, dependensi, struktur folder, atau dokumen), buat **satu file Markdown baru** di folder `changes/`.

### 3.1 Penamaan File
```
changes/YYYY-MM-DD_nama-perubahan.md
```
Contoh:
- `changes/2026-11-13_inisialisasi-claude-md.md`
- `changes/2026-11-15_setup-cargo-dan-gui-dasar.md`
- `changes/2026-11-18_fix-progress-bar-macet.md`

Jika ada lebih dari satu perubahan di hari yang sama, buat file terpisah untuk tiap perubahan. Jangan menimpa file catatan yang sudah ada.

### 3.2 Template Isi
```markdown
# [Judul Singkat Perubahan]

- **Tanggal:** YYYY-MM-DD
- **Jenis:** Fitur baru / Perbaikan bug / Refactor / Dokumentasi / Dependensi / Lainnya
- **Status:** Selesai / Sebagian / Perlu dicek

## Latar Belakang
Kenapa perubahan ini dilakukan (permintaan, bug, atau kebutuhan).

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| src/main.rs | Diubah | ... |
| src/downloader.rs | Dibuat | ... |

## Detail Perubahan
Penjelasan teknis singkat: apa yang ditambah/diubah dan bagaimana cara kerjanya.

## Dependensi
Crate yang ditambah/dihapus/diperbarui (atau "Tidak ada").

## Cara Menguji
Langkah untuk memastikan perubahan bekerja.

## Hasil Ukur Performa
RAM idle: ... MB · CPU idle: ... % (atau "tidak relevan").

## Catatan / Yang Perlu Dicek Ulang
Risiko, keterbatasan, atau hal yang belum selesai.
```

### 3.3 Ringkasan Perubahan
Selain file di `changes/`, tambahkan **satu baris** ke `CHANGELOG.md` di root projek (urutan terbaru di atas) berisi tanggal, judul, dan link ke file detailnya.

---

## 4. Lingkungan Pengembangan

| Kebutuhan | Keterangan |
|-----------|------------|
| OS | Windows 10/11 64-bit (laptop pemilik projek) |
| Rust | Toolchain **stable**, target `x86_64-pc-windows-msvc` (dipasang lewat `rustup`) |
| Build tools | Visual Studio Build Tools (komponen "Desktop development with C++") — dibutuhkan toolchain MSVC |
| Installer | Inno Setup 6 (baru dibutuhkan di Fase 8) |
| Editor | VS Code / Antigravity + ekstensi `rust-analyzer` dan Slint |

- Build, menjalankan aplikasi, dan pengukuran RAM/CPU dilakukan **di Windows pemilik projek**. Jika Claude tidak bisa menjalankan perintah langsung di Windows, Claude memberikan perintah yang harus dijalankan beserta hasil yang diharapkan, lalu pemilik projek melaporkan hasilnya.
- Sebelum Fase 1, pastikan `rustc --version` dan `cargo --version` sudah bisa dijalankan di terminal.
- Projek memakai git dengan remote di GitHub milik pemilik projek. `git commit` dan `git push` dijalankan oleh pemilik projek (atau oleh Claude hanya dengan izin, lihat Bagian 1).

---

## 5. Alur Kerja Setiap Tugas

1. Baca `CLAUDE.md` ini dan file yang relevan.
2. Jelaskan pemahaman + rencana singkat (Bagian 1) → **tunggu persetujuan**.
3. Kerjakan perubahan.
4. Jalankan pemeriksaan (jika toolchain tersedia):
   - `cargo fmt`
   - `cargo clippy -- -D warnings`
   - `cargo build`
   - `cargo test` (jika ada test)
5. Buat file catatan di `changes/` + update `CHANGELOG.md` (Bagian 3).
6. Laporkan singkat: apa yang dikerjakan, file apa saja yang dibuat/diubah, hasil build/test, dan bagian yang perlu dicek ulang.

---

## 6. Aturan Penulisan Kode

- **Bahasa:** nama variabel, fungsi, dan struct dalam **bahasa Inggris**; **komentar** dan **teks UI** dalam **Bahasa Indonesia**.
- Ikuti gaya standar Rust (`cargo fmt`), tanpa warning `clippy`.
- **Hindari `unwrap()` / `expect()`** di kode non-test; tangani error dengan `Result` dan tampilkan pesan yang ramah ke pengguna.
- **UI thread tidak boleh diblok.** Proses unduh berjalan di thread/task terpisah dan mengirim progress lewat channel.
- Pisahkan logika: GUI ↔ pengatur unduhan ↔ konfigurasi. Logika unduh harus bisa diuji tanpa GUI.
- Saat membangun argumen untuk yt-dlp/FFmpeg, berikan argumen sebagai **daftar terpisah** (bukan satu string shell) supaya URL atau nama file aneh tidak merusak perintah.
- Nama file hasil unduhan harus aman untuk Windows (tanpa karakter `\ / : * ? " < > |`).
- Fungsi publik dan bagian yang rumit diberi komentar penjelasan, karena projek ini juga dipakai sebagai bahan belajar Rust.
- Jangan menambah crate jika fungsinya bisa dikerjakan dengan mudah memakai library standar.

**Khusus Windows & Slint**
- Build release memakai `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` supaya tidak muncul jendela console hitam.
- Proses yt-dlp/FFmpeg dijalankan dengan flag `CREATE_NO_WINDOW` supaya tidak ada console yang berkedip saat mengunduh.
- Tampilan ditulis di file `.slint` (folder `ui/`); logika di Rust. Jangan menaruh logika bisnis di file `.slint`.
- Update UI dari thread lain hanya lewat mekanisme resmi Slint (mis. `slint::invoke_from_event_loop` atau `Weak` handle), bukan menyentuh komponen langsung dari thread pekerja.
- Path selalu memakai `PathBuf`/`Path`, bukan string yang digabung manual; dukung nama file dengan spasi dan karakter non-ASCII.

---

## 7. Struktur Folder (Rencana Awal)

Struktur ini boleh berkembang seiring fase, tetapi setiap perubahan struktur harus dikonfirmasi dulu.

```
DANload/                 ← D:\Code Project\DANload
├── CLAUDE.md            ← file ini
├── CHANGELOG.md         ← ringkasan semua perubahan
├── README.md            ← cara build & pakai
├── Cargo.toml
├── Cargo.lock           ← versi pasti semua crate (ikut di-commit)
├── build.rs             ← kompilasi .slint + tempel ikon ke .exe
├── .gitignore
├── changes/             ← satu file .md per perubahan
├── docs/
│   └── referensi-ui/    ← gambar acuan desain
├── assets/
│   ├── icon.ico         ← ikon file .exe
│   └── icon.png         ← ikon jendela/taskbar (dibuat dari icon.ico)
├── ui/
│   ├── app-window.slint ← jendela utama
│   ├── settings-page.slint ← halaman Pengaturan (status komponen, folder unduhan)
│   ├── components.slint ← komponen kecil (tombol, chip, kolom isian)
│   └── theme.slint      ← warna, ukuran, bentuk ikon
├── src/
│   ├── main.rs          ← titik masuk aplikasi
│   ├── app.rs           ← penghubung Slint ↔ logika (callback, thread pekerja)
│   ├── downloader.rs    ← memanggil yt-dlp, membaca progress
│   ├── formats.rs       ← daftar format & argumen per format
│   ├── platform.rs      ← deteksi YouTube/Facebook/Instagram dari URL
│   ├── library.rs       ← pustaka: daftar, rename, hapus, cek file hilang
│   ├── config.rs        ← baca/tulis config.json, folder unduhan bawaan
│   ├── deps.rs          ← cari yt-dlp/FFmpeg/ffprobe dan baca versinya
│   └── setup.rs         ← wizard setup: unduh, verifikasi, ekstrak, pasang komponen
├── installer/
│   └── DANload.iss      ← skrip installer Inno Setup
└── bin/                 ← (opsional, untuk pengujian lokal) yt-dlp.exe & ffmpeg.exe, TIDAK di-commit
```

---

## 8. Roadmap

| Fase | Isi | Status |
|------|-----|--------|
| 0 | Keputusan teknis: Slint, `std::thread`, JSON, lokasi yt-dlp/FFmpeg, Inno Setup | ✅ |
| 1 | Setup projek Cargo + Slint, jendela GUI kosong, ikon, **ukur RAM/CPU idle** (Bagian 2.6) | ⏳ Build debug di Windows berhasil; angka RAM/CPU versi rilis belum dilaporkan |
| 2A | Config, cek komponen (yt-dlp/FFmpeg/ffprobe), halaman Pengaturan, pilih folder unduhan | ⏳ Kode selesai; menunggu uji di Windows |
| 2B | **Wizard setup awal**: unduh & pasang yt-dlp/FFmpeg, pilih folder unduhan (Bagian 2.9) | ⬜ |
| 3 | Unduh video MP4 & audio MP3 dari YouTube + progress bar (setara app lama) | ⬜ |
| 4 | Tambah format lain (Bagian 2.4) | ⬜ |
| 5 | Dukungan Facebook & Instagram + penanganan error khusus tiap situs | ⬜ |
| 6 | **Pustaka:** daftar unduhan, cari/filter, rename, hapus (Recycle Bin) | ⬜ |
| 7 | Playlist, antrean, tombol batal | ⬜ |
| 8 | Poles UI agar sesuai Bagian 2.7, build release, **installer** (Bagian 2.9), README, uji di Windows bersih | ⬜ |

Perbarui kolom status setiap kali satu fase selesai (dan catat di `changes/`).

---

## 9. Definisi "Selesai"

Sebuah tugas dianggap selesai jika:
- [ ] `cargo build` berhasil tanpa error
- [ ] Tidak ada warning dari `cargo clippy`
- [ ] Untuk perubahan yang menyentuh UI/proses: RAM idle < 50 MB dan CPU idle < 5% (build release, dicek di Task Manager) — angka hasil ukur ditulis di file `changes/`
- [ ] Fitur sudah dicoba minimal dengan satu URL nyata (atau dijelaskan cara mengujinya)
- [ ] File catatan di `changes/` sudah dibuat dan `CHANGELOG.md` diperbarui
- [ ] Laporan singkat sudah disampaikan

---

## 10. Batasan & Etika

- Aplikasi ini untuk mengunduh konten yang **boleh diunduh** pengguna (konten sendiri, berlisensi bebas, atau untuk penggunaan pribadi sesuai aturan platform). Tampilkan pengingat singkat di halaman "Tentang".
- **Tidak** menambahkan fitur untuk membobol DRM, mengakses konten privat orang lain, atau menghindari pembatasan login secara tidak sah.
- Jangan menyimpan password, cookies, atau token pengguna di dalam kode maupun di repo.
- File besar (binary yt-dlp/FFmpeg, hasil build di `target/`) tidak di-commit; masukkan ke `.gitignore`.
- **Lisensi Slint:** Slint berlisensi ganda (GPLv3 atau lisensi *royalty-free* dengan syarat atribusi). Sebelum aplikasi dibagikan (Fase 8), cek syarat terbarunya di slint.dev dan penuhi atribusinya di halaman "Tentang". *(Belum diverifikasi ke sumber resmi.)*

---

## 11. Referensi

- Aplikasi lama (Python): `D:\Code Project\Youtube Downloader`
  - `youtube_downloader_gui.py` — versi GUI (Tkinter), acuan fitur dan alur UI
  - `youtube_downloader.py` — versi CLI
  - `icon.ico` — ikon yang bisa dipakai ulang (salin, jangan dipindah)
- Dokumentasi yt-dlp: opsi format (`-f`), `--merge-output-format`, `-x --audio-format`, `--remux-video`, `--recode-video`, `--newline`, `--progress-template`
