# Fase 5: Facebook & Instagram, Nama File Bisa Diatur, dan Nama Tanpa ID

- **Tanggal:** 2026-11-15
- **Jenis:** Fitur baru
- **Status:** Sebagian — kode selesai dan diuji di Linux dengan yt-dlp + FFmpeg asli memakai data Facebook/Instagram tiruan; menunggu uji di Windows dengan link sungguhan

## Latar Belakang
- Fase 5 roadmap: dukungan Facebook dan Instagram, plus penanganan error khusus tiap situs.
- Permintaan pemilik projek (2026-11-15):
  1. Nama file tanpa ID unik di belakang (`Judul.mp4`, bukan `Judul [abc123].mp4`).
  2. Pengguna bisa mengatur nama file sendiri. Kalau langsung mengunduh tanpa mengganti nama dan namanya sudah dipakai, `nama.mp4` menjadi `nama (2).mp4`.
  3. Postingan berisi beberapa video: unduh semua videonya.
- Tanpa ID, video berbeda bisa bernama sama. Contoh: Instagram menamai semua video `Video by <akun>`. Simulasi postingan Instagram berisi 3 video dengan yt-dlp asli menunjukkan video kedua tidak tersimpan, karena yt-dlp mengira file itu sudah ada. Karena itu nama akhir sekarang diberikan DANload.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| src/filename.rs | Dibuat | Pemeriksaan nama kustom (karakter terlarang Windows, nama perangkat seperti CON/NUL, panjang maks. 100 karakter, ekstensi yang ikut diketik dibuang) dan pemindahan file dengan nama unik `nama (2).ext` tanpa pernah menimpa; 6 unit test |
| src/downloader.rs | Diubah | Semua format lewat folder sementara (`hasil\<ID>\Judul.ext`); semua file dari satu link dikumpulkan; tiap file dikonversi jika perlu lalu dipindah dengan nama akhirnya; berhasil sebagian jika ada item yang dilewati; nomor video `(2/3)`; maks. 20 item per link; pesan error Facebook/Instagram; cek "file sudah ada" dari Fase 4 dihapus. Unit test baru: nama kembar dan nama kustom, postingan berisi beberapa video, konversi beberapa file, sukses tanpa file, pesan error Facebook/Instagram |
| src/platform.rs | Diubah | Facebook dan Instagram bisa diunduh. Cek jenis link: playlist/channel YouTube, profil/tag/story Instagram, dan beranda ditolak dengan pesan jelas; 1 unit test baru |
| src/formats.rs | Diubah | Keterangan format mengikuti platform: WEBM dari Facebook/Instagram biasanya diubah ulang (peringatan kuning), OPUS diubah dari AAC, MOV cukup disalin; 1 unit test baru |
| src/app.rs | Diubah | Kolom NAMA FILE (diperiksa langsung; dikosongkan setelah unduhan dimulai), nomor video di teks kemajuan, kartu hasil untuk beberapa file, keterangan format diperbarui saat platform diganti |
| src/main.rs | Diubah | Modul `filename` |
| ui/app-window.slint | Diubah | Kolom NAMA FILE dengan ekstensi di sebelahnya dan pesan jika namanya salah; tombol Unduh mati jika nama salah ("Perbaiki nama file dulu."); platform yang diganti dilaporkan ke Rust |
| README.md | Diubah | Status Fase 5, nama file, Facebook/Instagram, cara uji |
| CLAUDE.md | Diubah | 2.3, 2.4, 2.7, 2.8, 2.10, 4, 7, roadmap |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan

**Nama file**
- **Nama bawaan:** judul video, paling panjang 100 karakter, tanpa ID. Judul Facebook sering berupa teks postingan yang panjang, jadi bagian sisanya dipotong. Karakter terlarang di Windows diganti yt-dlp (`--windows-filenames`), mis. `|` menjadi `｜`.
- **Kolom NAMA FILE** di panel kanan:
  - Kosong (bawaan) berarti nama mengikuti judul video.
  - Ekstensi (`.mp4`, `.mp3`, …) tampil di sebelah kolom dan ditambahkan otomatis.
  - Nama yang salah langsung diberi pesan dan tombol Unduh mati:
    - berisi karakter `\ / : * ? " < > |`;
    - memakai nama perangkat Windows (CON, NUL, COM1, …);
    - lebih dari 100 karakter;
    - hanya berisi titik.
  - Kolom dikosongkan lagi setelah unduhan dimulai.
- **Nama sudah dipakai:** diberi nomor `nama (2).mp4`, `nama (3).mp4`, dst. File lain **tidak pernah ditimpa**. Nama "dipesan" dulu dengan membuat file kosong, sehingga file yang muncul tiba-tiba pun tidak tertimpa. Pemindahan dicoba ulang jika file dikunci antivirus.
- **Perubahan dari Fase 4:** pesan "Sudah ada di folder, tidak diunduh ulang" dihapus. Tanpa ID, DANload tidak bisa membedakan video yang sama dari video lain yang judulnya sama. Unduh ulang video yang sama sekarang menghasilkan `Judul (2).mp4`.

**Postingan berisi beberapa video** (mis. carousel Instagram)
- Semua video diunduh; tiap video menjadi file sendiri, mis. `Video by akun.mp4` dan `Video by akun (2).mp4`. Nama kustom juga diberi nomor: `Liburan.mp4`, `Liburan (2).mp4`.
- Foto dilewati. yt-dlp melaporkan "No video formats found!" dan kode keluar 1, tetapi unduhan tetap dinyatakan berhasil, dengan keterangan "1 item dilewati: Tidak ada video di link ini (mungkin hanya berisi foto)."
- Teks kemajuan memakai nomor:
  - "Mengunduh video (2/3) · 45%" (jumlah item di postingan);
  - "Mengubah ke WEBM (1/2) · 64%" (jumlah file yang dikonversi).
- Pengaman: paling banyak 20 item per link (`--playlist-items :20`). Satu postingan Instagram berisi paling banyak 20 item.

**Facebook dan Instagram**
- Kedua platform sekarang bisa diunduh, dengan semua format dari Fase 4.
- **Cek jenis link** sebelum mengunduh, supaya tidak mengunduh puluhan video sekaligus atau gagal tanpa alasan jelas:

  | Link | Hasil |
  |------|-------|
  | YouTube `playlist?list=`, `@channel`, `/channel/`, beranda, pencarian | Ditolak dengan pesan. Link `watch?v=…&list=…` tetap boleh (hanya videonya) |
  | Instagram profil, `/explore/` (tag), `/stories/` (butuh login), beranda | Ditolak dengan pesan |
  | Instagram `/p/`, `/reel/`, `/reels/`, `/tv/`, `/<akun>/p/…`, `/share/…` | Boleh |
  | Facebook | Semua link boleh kecuali beranda (bentuk link video Facebook sangat beragam) |

  Sebelumnya, link playlist atau channel YouTube akan mengunduh semua videonya.
- **Pesan error baru.** Teksnya diambil dari kode sumber yt-dlp 2026.10.26:

  | Error dari yt-dlp | Pesan di DANload |
  |-------------------|------------------|
  | Instagram membatasi akses tanpa login ("rate-limit") | "Instagram sedang membatasi akses tanpa login. Tunggu beberapa saat…" |
  | Akun privat ("who follow this account") | "Akun ini privat…" |
  | "empty media response" | "Instagram tidak memberikan data postingan ini…" |
  | Postingan foto ("There is no video in this post" / "No video formats found") | "Tidak ada video di link ini (mungkin hanya berisi foto)." |
  | Facebook "Cannot parse data" | "Facebook tidak memberikan data video ini…" |
  | Facebook "The video is not available, Facebook said" | "Video ini tidak tersedia menurut Facebook…" |
  | Butuh login (semua pesan yang menyarankan `--cookies`) | "Konten ini butuh login (akun), jadi belum bisa diunduh DANload." |
  | Instagram "Post isn't available" | Pesan "Video tidak tersedia" |

  Pesan YouTube yang juga menyarankan `--cookies` (bukan bot, dibatasi umur, privat, khusus anggota) tetap mendapat pesannya sendiri.
- **Keterangan format menyesuaikan platform.** Facebook dan Instagram umumnya hanya menyediakan H.264 + AAC, jadi:
  - WEBM: "biasanya … perlu diubah ulang ke VP9 + Opus: proses lebih lama" (kuning);
  - OPUS: diubah dari AAC;
  - MOV: "biasanya cukup disalin, jadi cepat".
- **Tanpa login/cookies** (sesuai roadmap, fitur tambahan nanti). Konten yang butuh login mendapat pesan jelas.

## Dependensi
Tidak ada crate baru (`Cargo.toml` dan `Cargo.lock` tidak berubah).

## Cara Menguji
1. `cargo test`: 101 test lolos di Linux, 4 diabaikan (butuh program asli). Di Windows jumlahnya lebih sedikit, karena beberapa test memakai skrip shell khusus Linux.
2. `cargo run` → panel kanan punya kolom **NAMA FILE** ("Otomatis dari judul video", `.mp4` di sebelahnya). Coba ketik `Liburan: Bali`: muncul pesan kuning dan Unduh mati. Ganti `Liburan Bali`, maka Unduh aktif.
3. Unduh satu video YouTube dua kali tanpa mengisi nama. Hasilnya `Judul.mp4` dan `Judul (2).mp4`.
4. Pilih **Facebook** → tempel link video Facebook → unduh MP4 dan MP3.
5. Pilih **Instagram**:
   - Tempel link profil: muncul pesan.
   - Tempel link reel/postingan, lalu unduh.
   - Coba postingan berisi beberapa video: kartu menampilkan "(1/3)", hasilnya beberapa file, dan foto dilewati.
6. Pilih WEBM saat Instagram/Facebook terpilih: keterangannya kuning. Pilih YouTube: keterangannya kembali biasa.
7. Tempel link playlist YouTube (`youtube.com/playlist?list=...`): muncul pesan dan Unduh mati.
8. Opsional, uji otomatis dengan program asli: `DANLOAD_TEST_URL` boleh link Facebook atau Instagram (lihat README).

Sudah diuji (Linux): `cargo fmt`, `clippy --all-targets` tanpa warning, 101 unit test. Dengan **yt-dlp 2026.10.26 + FFmpeg 6.1 asli** lewat server lokal:
- **Facebook tiruan** (DASH H.264 + HE-AAC, judul postingan ±140 karakter berisi emoji dan `|`): 12 format berhasil. Nama dipotong 100 karakter; WEBM di-encode ulang ke VP9 (8,6 detik untuk klip 10 detik 1080p); MOV hanya disalin.
- **Postingan Instagram tiruan** (2 video + 1 foto): 12 format × 2 file berhasil dengan nama `Video by akun_uji` dan `… (2)`; foto dilewati dengan keterangan.
- **YouTube** (regresi): 12 format; pemilihan format 20 kombinasi tetap sama.
- **Unduh ulang** menghasilkan `(2)` (atau `(3)`, `(4)` untuk postingan); nama kustom `Nama Kustom é.m4a`.
- **Batal** saat konversi postingan Instagram: berhenti 7 ms setelah Batal, tanpa sisa file.
- **Kodek** tiap file hasil diperiksa ffprobe; semua bisa dibaca FFmpeg tanpa error.
- **Tampilan (tangkapan layar):**
  - kolom nama, pesan nama salah, dan tombol mati;
  - link profil Instagram dan link playlist YouTube ditolak;
  - keterangan WEBM kuning untuk Instagram;
  - kartu "Menggabungkan video dan audio (1/3)", "Mengubah ke WEBM (1/2) · 64%", "Selesai · 2 file disimpan · … · 1 item dilewati: …";
  - kolom nama kosong lagi setelah unduhan dimulai.

Belum diuji:
- Facebook dan Instagram sungguhan (tidak bisa diakses dari workspace);
- YouTube sungguhan;
- semua hal khusus Windows.

## Hasil Ukur Performa
- Indikasi Linux (build release Fase 5): RAM 23,6 MB saat diam setelah dibuka, 24,3 MB setelah dua unduhan; CPU 0% saat diam. Sama dengan Fase 3–4 di Linux.
- Windows (Fase 3–5): **belum diukur**. Fase 2B di Windows: 9,8 MB, 0%.

## Catatan / Yang Perlu Dicek Ulang
- **Instagram tanpa login sering dibatasi.** yt-dlp punya pesan khusus untuk ini ("rate-limit", "empty media response"), jadi sebagian unduhan Instagram bisa gagal walau link-nya benar. Dukungan login/cookies adalah fitur tambahan terpisah.
- **Nama file Instagram** dari yt-dlp selalu `Video by <akun>`. Kalau ingin nama yang lebih jelas, isi kolom NAMA FILE.
- **Nama Facebook** berupa teks postingan (bisa berisi jumlah tayangan, mis. "9,7 rb tayangan · 352 reaksi | …").
- **"Sudah ada di folder" (Fase 4) diganti penomoran.** Video yang sama bisa tersimpan dua kali jika diunduh dua kali.
- **Nomor saat unduh vs konversi berbeda.** Nomor saat mengunduh memakai jumlah item di postingan (termasuk foto), sedangkan saat konversi memakai jumlah file video, jadi bisa berbeda ("(1/3)" lalu "(1/2)").
- **Batal saat DANload sedang mengubah video ke-2 dari sebuah postingan:** video ke-1 yang sudah dipindah tetap tersimpan di folder unduhan, walau kartunya menulis "Dibatalkan". Batal saat yt-dlp masih mengunduh tidak meninggalkan file.
