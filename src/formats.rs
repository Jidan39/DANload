//! Pilihan format unduhan dan argumen yt-dlp untuk tiap format (CLAUDE.md Bagian 2.4 dan 2.10).
//!
//! Ada dua jalur:
//! - **Langsung dari yt-dlp**: MP4, MKV, dan semua format audio. yt-dlp mengunduh,
//!   menggabungkan video + audio, atau mengambil audionya, lalu menaruh hasilnya di folder unduhan.
//! - **Lewat langkah konversi DANload** (`src/convert.rs`): WEBM, MOV, AVI, MPEG. yt-dlp
//!   menaruh hasil gabungannya di folder sementara, lalu DANload menyelesaikannya dengan FFmpeg
//!   sambil menampilkan kemajuan. Stream yang sudah cocok cukup disalin, tidak diubah ulang.
//!   Untuk WEBM, jika kodek dari sumber sudah cocok (VP9/AV1 + Opus), yt-dlp langsung
//!   menggabungkannya ke WEBM dan DANload hanya memindahkan filenya.
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

use serde::{Deserialize, Serialize};

use crate::platform::Platform;

/// Pilihan resolusi maksimum video, sesuai urutan di menu. `None` berarti terbaik.
pub const VIDEO_HEIGHTS: [Option<u32>; 7] = [
    None,
    Some(2160),
    Some(1440),
    Some(1080),
    Some(720),
    Some(480),
    Some(360),
];

/// Pilihan bitrate audio (kbps) untuk MP3 dan OGG, sesuai urutan di menu.
pub const AUDIO_BITRATES: [u32; 4] = [128, 192, 256, 320];

/// Bitrate audio bawaan, sama dengan aplikasi lama.
pub const DEFAULT_AUDIO_BITRATE: u32 = 192;

/// Resolusi tertinggi untuk format yang selalu diubah ulang (AVI dan MPEG).
/// Di atas ini prosesnya sangat lama, dan kodek lama (Xvid, MPEG-2) memang tidak dibuat untuk itu.
pub const REENCODE_MAX_HEIGHT: u32 = 1080;

/// Pilihan resolusi di menu untuk format yang dibatasi [`REENCODE_MAX_HEIGHT`].
pub const CAPPED_VIDEO_HEIGHTS: [Option<u32>; 4] =
    [Some(REENCODE_MAX_HEIGHT), Some(720), Some(480), Some(360)];

/// Bitrate cadangan untuk M4A/OPUS. Hanya dipakai jika sumbernya tidak punya audio AAC/Opus
/// sehingga yt-dlp terpaksa mengubahnya.
const FALLBACK_AUDIO_BITRATE: u32 = 192;

/// Format video hasil unduhan, sesuai urutan di menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoFormat {
    #[default]
    Mp4,
    Mkv,
    Webm,
    Mov,
    Avi,
    Mpeg,
}

impl VideoFormat {
    pub const ALL: [VideoFormat; 6] = [
        VideoFormat::Mp4,
        VideoFormat::Mkv,
        VideoFormat::Webm,
        VideoFormat::Mov,
        VideoFormat::Avi,
        VideoFormat::Mpeg,
    ];

    /// Nama yang tampil di menu, mis. `MP4`.
    pub fn name(self) -> &'static str {
        match self {
            VideoFormat::Mp4 => "MP4",
            VideoFormat::Mkv => "MKV",
            VideoFormat::Webm => "WEBM",
            VideoFormat::Mov => "MOV",
            VideoFormat::Avi => "AVI",
            VideoFormat::Mpeg => "MPEG",
        }
    }

    /// Ekstensi file hasil (tanpa titik).
    pub fn extension(self) -> &'static str {
        match self {
            VideoFormat::Mp4 => "mp4",
            VideoFormat::Mkv => "mkv",
            VideoFormat::Webm => "webm",
            VideoFormat::Mov => "mov",
            VideoFormat::Avi => "avi",
            VideoFormat::Mpeg => "mpg",
        }
    }

    /// `true` jika hasilnya dibuat oleh langkah konversi DANload (bukan langsung oleh yt-dlp).
    pub fn needs_conversion(self) -> bool {
        matches!(
            self,
            VideoFormat::Webm | VideoFormat::Mov | VideoFormat::Avi | VideoFormat::Mpeg
        )
    }

    /// `true` jika videonya selalu diubah ulang (lambat). Dipakai untuk peringatan di UI.
    pub fn always_reencodes(self) -> bool {
        matches!(self, VideoFormat::Avi | VideoFormat::Mpeg)
    }

    /// Batas resolusi khusus format ini, jika ada.
    pub fn height_cap(self) -> Option<u32> {
        self.always_reencodes().then_some(REENCODE_MAX_HEIGHT)
    }

    /// Keterangan singkat di bawah pilihan format untuk unduhan dari `platform`.
    ///
    /// YouTube menyediakan VP9/AV1 + Opus selain H.264 + AAC, sedangkan Facebook dan Instagram
    /// umumnya hanya H.264 + AAC. Karena itu WEBM dari Facebook/Instagram biasanya harus
    /// diubah ulang, sedangkan MOV cukup disalin.
    pub fn hint(self, platform: Platform) -> String {
        let youtube = platform == Platform::YouTube;
        let text = match self {
            VideoFormat::Mp4 if youtube => {
                "Paling mudah diputar di mana saja: H.264 + AAC diutamakan. Di atas 1080p biasanya VP9/AV1."
            }
            VideoFormat::Mp4 => "Paling mudah diputar di mana saja (H.264 + AAC).",
            VideoFormat::Mkv if youtube => {
                "Kualitas terbaik dari sumber apa adanya (AV1/VP9, termasuk HDR jika ada)."
            }
            VideoFormat::Mkv => "Kualitas terbaik dari sumber apa adanya, tanpa diubah ulang.",
            VideoFormat::Webm if youtube => {
                "Format asli YouTube (VP9/AV1 + Opus), cocok untuk web."
            }
            VideoFormat::Webm => {
                return format!(
                    "{} biasanya memakai H.264, jadi video sering perlu diubah ulang ke VP9 + Opus: proses lebih lama.",
                    platform.name()
                );
            }
            VideoFormat::Mov if youtube => {
                "H.264 + AAC untuk perangkat Apple dan aplikasi edit; di YouTube biasanya paling tinggi 1080p. Jika tidak tersedia, video diubah ulang (lebih lama)."
            }
            VideoFormat::Mov => {
                "H.264 + AAC untuk perangkat Apple dan aplikasi edit; biasanya cukup disalin, jadi cepat."
            }
            VideoFormat::Avi => {
                "Selalu diubah ulang ke Xvid + MP3: proses lebih lama dan file lebih besar. Paling tinggi 1080p."
            }
            VideoFormat::Mpeg => {
                "Selalu diubah ulang ke MPEG-2: proses lebih lama dan file lebih besar. Paling tinggi 1080p."
            }
        };
        text.to_string()
    }

    /// `true` jika format ini (kemungkinan besar) diubah ulang videonya untuk unduhan dari
    /// `platform`, sehingga lambat. Dipakai untuk mewarnai keterangan sebagai peringatan.
    pub fn is_slow_for(self, platform: Platform) -> bool {
        self.always_reencodes() || (self == VideoFormat::Webm && platform != Platform::YouTube)
    }

    /// Posisi di menu.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|format| *format == self)
            .unwrap_or(0)
    }

    /// Kebalikan dari [`VideoFormat::index`]. Indeks yang tidak dikenal dianggap MP4.
    pub fn from_index(index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
            .unwrap_or_default()
    }
}

/// Format audio hasil unduhan, sesuai urutan di menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    #[default]
    Mp3,
    M4a,
    Opus,
    Ogg,
    Flac,
    Wav,
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 6] = [
        AudioFormat::Mp3,
        AudioFormat::M4a,
        AudioFormat::Opus,
        AudioFormat::Ogg,
        AudioFormat::Flac,
        AudioFormat::Wav,
    ];

    /// Nama yang tampil di menu, mis. `MP3`.
    pub fn name(self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "MP3",
            AudioFormat::M4a => "M4A",
            AudioFormat::Opus => "OPUS",
            AudioFormat::Ogg => "OGG",
            AudioFormat::Flac => "FLAC",
            AudioFormat::Wav => "WAV",
        }
    }

    /// Ekstensi file hasil (tanpa titik). Sama dengan yang dibuat yt-dlp.
    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "mp3",
            AudioFormat::M4a => "m4a",
            AudioFormat::Opus => "opus",
            AudioFormat::Ogg => "ogg",
            AudioFormat::Flac => "flac",
            AudioFormat::Wav => "wav",
        }
    }

    /// Nilai untuk `--audio-format` yt-dlp. OGG ditulis `vorbis` (nama kodeknya).
    fn yt_dlp_codec(self) -> &'static str {
        match self {
            AudioFormat::Ogg => "vorbis",
            other => other.extension(),
        }
    }

    /// Jenis pilihan kualitas untuk format ini.
    pub fn quality(self) -> AudioQuality {
        match self {
            AudioFormat::Mp3 | AudioFormat::Ogg => AudioQuality::Bitrate,
            AudioFormat::M4a | AudioFormat::Opus => AudioQuality::Original,
            AudioFormat::Flac => AudioQuality::Lossless,
            AudioFormat::Wav => AudioQuality::Uncompressed,
        }
    }

    /// Keterangan singkat di bawah pilihan format untuk unduhan dari `platform`.
    pub fn hint(self, platform: Platform) -> &'static str {
        match self {
            AudioFormat::Mp3 => "Paling umum, bisa diputar di hampir semua perangkat.",
            AudioFormat::M4a => "Audio AAC asli dari sumber, biasanya tanpa diubah ulang.",
            AudioFormat::Opus if platform == Platform::YouTube => {
                "Audio Opus asli dari sumber, biasanya tanpa diubah ulang. Ukurannya kecil."
            }
            AudioFormat::Opus => {
                "Audio sumber (biasanya AAC) diubah ulang ke Opus. Ukurannya kecil."
            }
            AudioFormat::Ogg => "Ogg Vorbis, format terbuka. Audio sumber diubah ulang.",
            AudioFormat::Flac => {
                "Lossless, tetapi kualitasnya tetap sebatas sumber; ukuran file jauh lebih besar."
            }
            AudioFormat::Wav => {
                "Tanpa kompresi: ukuran file sangat besar, kualitasnya tetap sebatas sumber."
            }
        }
    }

    /// Posisi di menu.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|format| *format == self)
            .unwrap_or(0)
    }

    /// Kebalikan dari [`AudioFormat::index`]. Indeks yang tidak dikenal dianggap MP3.
    pub fn from_index(index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
            .unwrap_or_default()
    }
}

/// Jenis pilihan kualitas sebuah format audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioQuality {
    /// Bitrate bisa dipilih (MP3, OGG).
    Bitrate,
    /// Mengikuti sumber, tanpa diubah ulang (M4A, OPUS).
    Original,
    /// Tanpa kehilangan data (FLAC).
    Lossless,
    /// Tanpa kompresi (WAV).
    Uncompressed,
}

impl AudioQuality {
    /// Teks untuk format yang kualitasnya tidak bisa dipilih.
    pub fn fixed_label(self) -> Option<&'static str> {
        match self {
            AudioQuality::Bitrate => None,
            AudioQuality::Original => Some("Asli"),
            AudioQuality::Lossless => Some("Lossless"),
            AudioQuality::Uncompressed => Some("Tanpa kompresi"),
        }
    }
}

/// Format hasil unduhan beserta kualitasnya.
///
/// Bisa disimpan di JSON (antrean, `queue.json`), mis.
/// `{"mode": "video", "format": "mp4", "max_height": 1080}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum OutputFormat {
    /// Video. `max_height`: batas resolusi (mis. 1080), `None` = terbaik.
    Video {
        format: VideoFormat,
        max_height: Option<u32>,
    },
    /// Audio. `bitrate_kbps` hanya dipakai format yang bitrate-nya bisa dipilih.
    Audio {
        format: AudioFormat,
        bitrate_kbps: u32,
    },
}

impl OutputFormat {
    /// Nama format, mis. `MP4` atau `FLAC`.
    pub fn name(self) -> &'static str {
        match self {
            OutputFormat::Video { format, .. } => format.name(),
            OutputFormat::Audio { format, .. } => format.name(),
        }
    }

    /// Ekstensi file hasil (tanpa titik).
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Video { format, .. } => format.extension(),
            OutputFormat::Audio { format, .. } => format.extension(),
        }
    }

    /// Label singkat untuk ditampilkan, mis. `MP4 · 1080p`, `MP3 · 192 kbps`, `FLAC · Lossless`.
    /// Untuk AVI/MPEG, resolusi yang ditampilkan sudah memperhitungkan batas 1080p.
    pub fn label(self) -> String {
        let quality = match self {
            OutputFormat::Video { .. } => height_label(self.effective_max_height()),
            OutputFormat::Audio {
                format,
                bitrate_kbps,
            } => match format.quality().fixed_label() {
                Some(label) => label.to_string(),
                None => bitrate_label(bitrate_kbps),
            },
        };
        format!("{} · {quality}", self.name())
    }

    /// Batas resolusi yang benar-benar dipakai (pilihan pengguna, dibatasi aturan format).
    pub fn effective_max_height(self) -> Option<u32> {
        match self {
            OutputFormat::Video { format, max_height } => match (max_height, format.height_cap()) {
                (Some(height), Some(cap)) => Some(height.min(cap)),
                (None, cap) => cap,
                (height, None) => height,
            },
            OutputFormat::Audio { .. } => None,
        }
    }

    /// Keterangan singkat untuk format ini (tampil di bawah pilihan format).
    pub fn hint(self, platform: Platform) -> String {
        match self {
            OutputFormat::Video { format, .. } => format.hint(platform),
            OutputFormat::Audio { format, .. } => format.hint(platform).to_string(),
        }
    }

    /// `true` jika keterangannya peringatan: videonya (kemungkinan besar) diubah ulang, jadi lambat.
    pub fn hint_is_warning(self, platform: Platform) -> bool {
        matches!(self, OutputFormat::Video { format, .. } if format.is_slow_for(platform))
    }

    /// Format video yang harus dibuat oleh langkah konversi DANload, jika ada.
    pub fn conversion(self) -> Option<VideoFormat> {
        match self {
            OutputFormat::Video { format, .. } if format.needs_conversion() => Some(format),
            _ => None,
        }
    }

    /// Argumen yt-dlp untuk memilih dan membentuk format ini.
    ///
    /// Batas resolusi memakai urutan `-S res:N`, bukan saringan `[height<=N]`: `res` adalah sisi
    /// terpendek gambar, jadi video vertikal (Shorts) 1080x1920 tetap dianggap 1080p, dan tidak
    /// pernah ada error "format tidak tersedia" karena saringan.
    pub fn args(self) -> Vec<String> {
        match self {
            OutputFormat::Video { format, .. } => self.video_args(format),
            OutputFormat::Audio {
                format,
                bitrate_kbps,
            } => audio_args(format, bitrate_kbps),
        }
    }

    fn video_args(self, format: VideoFormat) -> Vec<String> {
        let resolution = match self.effective_max_height() {
            Some(height) => format!("res:{height}"),
            None => "res".to_string(),
        };
        // Urutan "paling mudah diputar" (keputusan Fase 4): resolusi dan fps tetap diutamakan,
        // lalu di resolusi yang sama pilih H.264 (`+vcodec:avc` berarti H.264 > H.265 > VP9 > AV1),
        // gambar SDR (`+hdr`), dan audio AAC (`acodec:aac`). `source` ada di depan kodek supaya
        // format yang ditandai bermasalah oleh yt-dlp tetap di belakang.
        let compatible = format!("{resolution},fps,source,+vcodec:avc,+hdr,acodec:aac");
        let (selector, sort, merge) = match format {
            VideoFormat::Mp4 => ("bestvideo+bestaudio/best", compatible, "mp4"),
            // Kualitas terbaik apa pun kodeknya (urutan bawaan yt-dlp); MKV bisa menampung semuanya.
            VideoFormat::Mkv => ("bestvideo+bestaudio/best", resolution, "mkv"),
            // Format asli YouTube (VP9 + Opus). `webm/mkv`: yt-dlp langsung menggabung ke WEBM
            // jika kodeknya cocok; jika tidak, ke MKV, lalu DANload yang mengubahnya.
            VideoFormat::Webm => (
                "bestvideo[ext=webm]+bestaudio[ext=webm]/bestvideo+bestaudio/best",
                format!("{resolution},fps,source,+hdr"),
                "webm/mkv",
            ),
            // H.264 + AAC (bahkan jika resolusinya lebih rendah) digabung ke MP4 dulu, lalu cukup
            // disalin DANload ke MOV. `mov/mkv` tidak bisa dipakai: yt-dlp selalu memilih MKV.
            VideoFormat::Mov => (
                "bestvideo[vcodec^=avc1]+bestaudio[acodec^=mp4a]/bestvideo+bestaudio/best",
                compatible,
                "mp4/mkv",
            ),
            // Selalu diubah ulang; sumber H.264 paling ringan dibaca FFmpeg.
            VideoFormat::Avi | VideoFormat::Mpeg => ("bestvideo+bestaudio/best", compatible, "mkv"),
        };
        let mut args = vec![
            "-f".to_string(),
            selector.to_string(),
            "-S".to_string(),
            sort,
            "--merge-output-format".to_string(),
            merge.to_string(),
        ];
        if format == VideoFormat::Mkv {
            // Jika yang terunduh satu file utuh (tanpa digabung), tetap jadikan MKV tanpa diubah ulang.
            args.extend(["--remux-video".to_string(), "mkv".to_string()]);
        }
        args
    }
}

fn audio_args(format: AudioFormat, bitrate_kbps: u32) -> Vec<String> {
    let selector = match format {
        AudioFormat::M4a => "bestaudio[acodec^=mp4a]/bestaudio/best",
        AudioFormat::Opus => "bestaudio[acodec=opus]/bestaudio/best",
        _ => "bestaudio/best",
    };
    let mut args = vec![
        "-f".to_string(),
        selector.to_string(),
        "--extract-audio".to_string(),
        "--audio-format".to_string(),
        format.yt_dlp_codec().to_string(),
    ];
    let bitrate = match format.quality() {
        AudioQuality::Bitrate => Some(bitrate_kbps),
        AudioQuality::Original => Some(FALLBACK_AUDIO_BITRATE),
        AudioQuality::Lossless | AudioQuality::Uncompressed => None,
    };
    if let Some(kbps) = bitrate {
        args.extend(["--audio-quality".to_string(), format!("{kbps}K")]);
    }
    if format == AudioFormat::Flac {
        // Tanpa ini, FLAC dari audio Opus/AAC menjadi 24-bit: file jauh lebih besar tanpa
        // tambahan kualitas (sumbernya sudah terkompresi), dan tidak semua pemutar mendukungnya.
        // 16-bit sama dengan WAV dan CD audio.
        args.extend([
            "--postprocessor-args".to_string(),
            "ExtractAudio+ffmpeg_o:-sample_fmt s16".to_string(),
        ]);
    }
    args.push("--embed-metadata".to_string());
    args
}

/// Jenis pilihan kualitas yang tampil di samping pilihan format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualityMenu {
    /// Resolusi maksimum; isinya pilihan yang tampil di menu (AVI/MPEG hanya sampai 1080p).
    Resolution(&'static [Option<u32>]),
    /// Bitrate (MP3, OGG).
    Bitrate,
    /// Tidak bisa dipilih; hanya menampilkan teks ini.
    Fixed(&'static str),
}

/// Pilihan kualitas dari menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityChoice {
    /// Resolusi maksimum video; `None` = terbaik.
    MaxHeight(Option<u32>),
    /// Bitrate audio (kbps).
    Bitrate(u32),
}

impl QualityMenu {
    /// Menu kualitas untuk format terpilih.
    pub fn for_format(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Video { format, .. } => {
                QualityMenu::Resolution(match format.height_cap() {
                    Some(_) => &CAPPED_VIDEO_HEIGHTS,
                    None => &VIDEO_HEIGHTS,
                })
            }
            OutputFormat::Audio { format, .. } => match format.quality().fixed_label() {
                Some(label) => QualityMenu::Fixed(label),
                None => QualityMenu::Bitrate,
            },
        }
    }

    /// Judul kolom kualitas.
    pub fn title(&self) -> &'static str {
        match self {
            QualityMenu::Resolution(_) => "RESOLUSI MAKS.",
            QualityMenu::Bitrate => "BITRATE",
            QualityMenu::Fixed(_) => "KUALITAS",
        }
    }

    /// Label pilihan di menu.
    pub fn labels(&self) -> Vec<String> {
        match self {
            QualityMenu::Resolution(heights) => heights.iter().copied().map(height_label).collect(),
            QualityMenu::Bitrate => AUDIO_BITRATES.into_iter().map(bitrate_label).collect(),
            QualityMenu::Fixed(label) => vec![(*label).to_string()],
        }
    }

    /// Posisi pilihan yang sedang berlaku untuk `format`. Untuk AVI/MPEG, resolusi di atas
    /// 1080p (termasuk "Terbaik") ditampilkan sebagai 1080p.
    pub fn index_for(&self, format: OutputFormat) -> usize {
        match (self, format) {
            (QualityMenu::Resolution(heights), OutputFormat::Video { .. }) => {
                let height = format.effective_max_height();
                heights
                    .iter()
                    .position(|candidate| *candidate == height)
                    .unwrap_or(0)
            }
            (QualityMenu::Bitrate, OutputFormat::Audio { bitrate_kbps, .. }) => {
                audio_bitrate_index(bitrate_kbps)
            }
            _ => 0,
        }
    }

    /// Pilihan di posisi `index` (dari UI). `None` jika menu ini tidak bisa dipilih
    /// atau indeksnya di luar daftar.
    pub fn choice(&self, index: i32) -> Option<QualityChoice> {
        let index = usize::try_from(index).ok()?;
        match self {
            QualityMenu::Resolution(heights) => {
                heights.get(index).copied().map(QualityChoice::MaxHeight)
            }
            QualityMenu::Bitrate => AUDIO_BITRATES
                .get(index)
                .copied()
                .map(QualityChoice::Bitrate),
            QualityMenu::Fixed(_) => None,
        }
    }

    /// `false` jika pilihannya hanya satu dan tidak bisa diubah.
    pub fn is_selectable(&self) -> bool {
        !matches!(self, QualityMenu::Fixed(_))
    }
}

/// Label resolusi, mis. `1080p`; `None` → `Terbaik`.
pub fn height_label(height: Option<u32>) -> String {
    match height {
        Some(height) => format!("{height}p"),
        None => "Terbaik".to_string(),
    }
}

/// Label bitrate, mis. `192 kbps`.
pub fn bitrate_label(kbps: u32) -> String {
    format!("{kbps} kbps")
}

/// Posisi `kbps` di menu bitrate. Nilai yang tidak ada di menu dianggap bitrate bawaan.
pub fn audio_bitrate_index(kbps: u32) -> usize {
    let find = |value: u32| {
        AUDIO_BITRATES
            .iter()
            .position(|candidate| *candidate == value)
    };
    find(kbps)
        .or_else(|| find(DEFAULT_AUDIO_BITRATE))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(format: VideoFormat, max_height: Option<u32>) -> OutputFormat {
        OutputFormat::Video { format, max_height }
    }

    fn audio(format: AudioFormat, bitrate_kbps: u32) -> OutputFormat {
        OutputFormat::Audio {
            format,
            bitrate_kbps,
        }
    }

    /// Nilai argumen setelah `flag`, mis. nilai `-f`.
    fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].as_str())
    }

    #[test]
    fn mp4_prefers_h264_at_the_same_resolution() {
        let args = video(VideoFormat::Mp4, Some(1080)).args();
        assert_eq!(value_of(&args, "-f"), Some("bestvideo+bestaudio/best"));
        assert_eq!(
            value_of(&args, "-S"),
            Some("res:1080,fps,source,+vcodec:avc,+hdr,acodec:aac")
        );
        assert_eq!(value_of(&args, "--merge-output-format"), Some("mp4"));
        let best = video(VideoFormat::Mp4, None).args();
        assert_eq!(
            value_of(&best, "-S"),
            Some("res,fps,source,+vcodec:avc,+hdr,acodec:aac")
        );
        assert_eq!(value_of(&best, "--remux-video"), None);
    }

    #[test]
    fn mkv_keeps_yt_dlp_best_quality_order() {
        let mkv = video(VideoFormat::Mkv, Some(720)).args();
        assert_eq!(value_of(&mkv, "-f"), Some("bestvideo+bestaudio/best"));
        assert_eq!(value_of(&mkv, "-S"), Some("res:720"));
        assert_eq!(value_of(&mkv, "--merge-output-format"), Some("mkv"));
        assert_eq!(value_of(&mkv, "--remux-video"), Some("mkv"));
    }

    #[test]
    fn converted_formats_use_the_right_intermediate_container() {
        let merge = |format| {
            value_of(&video(format, None).args(), "--merge-output-format").map(str::to_string)
        };
        // WEBM: langsung WEBM jika kodeknya cocok; MOV: MP4 (H.264 + AAC) lalu disalin.
        assert_eq!(merge(VideoFormat::Webm).as_deref(), Some("webm/mkv"));
        assert_eq!(merge(VideoFormat::Mov).as_deref(), Some("mp4/mkv"));
        assert_eq!(merge(VideoFormat::Avi).as_deref(), Some("mkv"));
        assert_eq!(merge(VideoFormat::Mpeg).as_deref(), Some("mkv"));
        for format in [
            VideoFormat::Webm,
            VideoFormat::Mov,
            VideoFormat::Avi,
            VideoFormat::Mpeg,
        ] {
            assert_eq!(video(format, None).conversion(), Some(format));
        }
        assert_eq!(video(VideoFormat::Mp4, None).conversion(), None);
        assert_eq!(video(VideoFormat::Mkv, None).conversion(), None);
        assert_eq!(audio(AudioFormat::Flac, 0).conversion(), None);

        let webm = video(VideoFormat::Webm, Some(1440)).args();
        assert!(value_of(&webm, "-f").is_some_and(|f| f.starts_with("bestvideo[ext=webm]")));
        assert_eq!(value_of(&webm, "-S"), Some("res:1440,fps,source,+hdr"));
        let mov = video(VideoFormat::Mov, None).args();
        assert!(value_of(&mov, "-f").is_some_and(|f| f.starts_with("bestvideo[vcodec^=avc1]")));
    }

    #[test]
    fn avi_and_mpeg_are_capped_at_1080p() {
        assert_eq!(
            video(VideoFormat::Avi, None).effective_max_height(),
            Some(1080)
        );
        assert_eq!(
            video(VideoFormat::Mpeg, Some(2160)).effective_max_height(),
            Some(1080)
        );
        assert_eq!(
            video(VideoFormat::Avi, Some(720)).effective_max_height(),
            Some(720)
        );
        assert_eq!(video(VideoFormat::Mov, None).effective_max_height(), None);
        assert_eq!(
            value_of(&video(VideoFormat::Avi, None).args(), "-S"),
            Some("res:1080,fps,source,+vcodec:avc,+hdr,acodec:aac")
        );
        assert_eq!(video(VideoFormat::Mpeg, None).label(), "MPEG · 1080p");
    }

    #[test]
    fn audio_args_per_format() {
        let mp3 = audio(AudioFormat::Mp3, 320).args();
        assert_eq!(value_of(&mp3, "--audio-format"), Some("mp3"));
        assert_eq!(value_of(&mp3, "--audio-quality"), Some("320K"));
        assert!(mp3.contains(&"--extract-audio".to_string()));
        assert!(mp3.contains(&"--embed-metadata".to_string()));

        let ogg = audio(AudioFormat::Ogg, 128).args();
        assert_eq!(value_of(&ogg, "--audio-format"), Some("vorbis"));
        assert_eq!(value_of(&ogg, "--audio-quality"), Some("128K"));

        let m4a = audio(AudioFormat::M4a, 320).args();
        assert_eq!(
            value_of(&m4a, "-f"),
            Some("bestaudio[acodec^=mp4a]/bestaudio/best")
        );
        // Bitrate pilihan pengguna tidak dipakai; hanya cadangan jika terpaksa diubah.
        assert_eq!(value_of(&m4a, "--audio-quality"), Some("192K"));

        let opus = audio(AudioFormat::Opus, 128).args();
        assert_eq!(
            value_of(&opus, "-f"),
            Some("bestaudio[acodec=opus]/bestaudio/best")
        );

        for format in [AudioFormat::Flac, AudioFormat::Wav] {
            let args = audio(format, 320).args();
            assert_eq!(value_of(&args, "--audio-format"), Some(format.extension()));
            assert_eq!(value_of(&args, "--audio-quality"), None);
        }
        // FLAC dibuat 16-bit (seperti WAV), bukan 24-bit.
        assert_eq!(
            value_of(
                &audio(AudioFormat::Flac, 192).args(),
                "--postprocessor-args"
            ),
            Some("ExtractAudio+ffmpeg_o:-sample_fmt s16")
        );
        assert_eq!(
            value_of(&audio(AudioFormat::Wav, 192).args(), "--postprocessor-args"),
            None
        );
    }

    #[test]
    fn labels() {
        assert_eq!(video(VideoFormat::Mp4, None).label(), "MP4 · Terbaik");
        assert_eq!(video(VideoFormat::Webm, Some(720)).label(), "WEBM · 720p");
        assert_eq!(audio(AudioFormat::Mp3, 192).label(), "MP3 · 192 kbps");
        assert_eq!(audio(AudioFormat::Opus, 192).label(), "OPUS · Asli");
        assert_eq!(audio(AudioFormat::Flac, 192).label(), "FLAC · Lossless");
        assert_eq!(audio(AudioFormat::Wav, 192).label(), "WAV · Tanpa kompresi");
        assert_eq!(video(VideoFormat::Mpeg, None).extension(), "mpg");
        assert_eq!(audio(AudioFormat::Ogg, 192).extension(), "ogg");
    }

    #[test]
    fn quality_menu_follows_format() {
        let full = QualityMenu::for_format(video(VideoFormat::Mp4, Some(720)));
        assert_eq!(full, QualityMenu::Resolution(&VIDEO_HEIGHTS));
        assert_eq!(full.title(), "RESOLUSI MAKS.");
        assert_eq!(full.labels()[0], "Terbaik");
        assert_eq!(full.index_for(video(VideoFormat::Mp4, Some(720))), 4);
        assert_eq!(full.choice(0), Some(QualityChoice::MaxHeight(None)));
        assert_eq!(full.choice(3), Some(QualityChoice::MaxHeight(Some(1080))));

        // AVI/MPEG: menu dimulai dari 1080p; "Terbaik" dan 2160p tampil sebagai 1080p.
        let capped = QualityMenu::for_format(video(VideoFormat::Avi, None));
        assert_eq!(capped.labels(), vec!["1080p", "720p", "480p", "360p"]);
        assert_eq!(capped.index_for(video(VideoFormat::Avi, None)), 0);
        assert_eq!(capped.index_for(video(VideoFormat::Mpeg, Some(2160))), 0);
        assert_eq!(capped.index_for(video(VideoFormat::Avi, Some(480))), 2);
        assert_eq!(capped.choice(1), Some(QualityChoice::MaxHeight(Some(720))));
        assert_eq!(capped.choice(4), None);

        let bitrate = QualityMenu::for_format(audio(AudioFormat::Ogg, 256));
        assert_eq!(bitrate, QualityMenu::Bitrate);
        assert_eq!(bitrate.index_for(audio(AudioFormat::Ogg, 256)), 2);
        assert!(bitrate.is_selectable());
        assert_eq!(bitrate.choice(3), Some(QualityChoice::Bitrate(320)));
        assert_eq!(bitrate.choice(-1), None);

        let fixed = QualityMenu::for_format(audio(AudioFormat::M4a, 256));
        assert_eq!(fixed, QualityMenu::Fixed("Asli"));
        assert_eq!(fixed.title(), "KUALITAS");
        assert_eq!(fixed.labels(), vec!["Asli".to_string()]);
        assert_eq!(fixed.index_for(audio(AudioFormat::M4a, 256)), 0);
        assert!(!fixed.is_selectable());
        assert_eq!(fixed.choice(0), None);
    }

    #[test]
    fn menu_indexes_fall_back_safely() {
        let menu = QualityMenu::for_format(video(VideoFormat::Mp4, None));
        assert_eq!(menu.index_for(video(VideoFormat::Mp4, None)), 0);
        assert_eq!(menu.index_for(video(VideoFormat::Mkv, Some(1080))), 3);
        assert_eq!(menu.index_for(video(VideoFormat::Mp4, Some(999))), 0);
        assert_eq!(audio_bitrate_index(320), 3);
        assert_eq!(audio_bitrate_index(0), 1); // tidak dikenal → 192 kbps
        assert_eq!(VideoFormat::from_index(4), VideoFormat::Avi);
        assert_eq!(VideoFormat::from_index(99), VideoFormat::Mp4);
        assert_eq!(VideoFormat::from_index(-1), VideoFormat::Mp4);
        assert_eq!(AudioFormat::from_index(5), AudioFormat::Wav);
        assert_eq!(AudioFormat::Wav.index(), 5);
        for format in VideoFormat::ALL {
            assert_eq!(VideoFormat::from_index(format.index() as i32), format);
        }
        for format in AudioFormat::ALL {
            assert_eq!(AudioFormat::from_index(format.index() as i32), format);
        }
    }

    #[test]
    fn every_format_has_a_hint_and_slow_ones_warn() {
        for platform in Platform::ALL {
            for format in VideoFormat::ALL {
                let output = video(format, None);
                assert!(!output.hint(platform).is_empty(), "{format:?}");
                let slow = format.always_reencodes()
                    || (format == VideoFormat::Webm && platform != Platform::YouTube);
                assert_eq!(
                    output.hint_is_warning(platform),
                    slow,
                    "{format:?} {platform:?}"
                );
            }
            for format in AudioFormat::ALL {
                let output = audio(format, 192);
                assert!(!output.hint(platform).is_empty(), "{format:?}");
                assert!(!output.hint_is_warning(platform));
            }
        }
        assert!(
            VideoFormat::Avi
                .hint(Platform::YouTube)
                .contains("lebih lama")
        );
        assert!(VideoFormat::Mpeg.hint(Platform::Facebook).contains("1080p"));
        assert!(
            AudioFormat::Flac
                .hint(Platform::Instagram)
                .contains("sumber")
        );
    }

    #[test]
    fn hints_follow_what_each_platform_provides() {
        // YouTube punya VP9 + Opus asli; Facebook/Instagram umumnya H.264 + AAC.
        assert!(
            VideoFormat::Webm
                .hint(Platform::YouTube)
                .contains("asli YouTube")
        );
        let webm = VideoFormat::Webm.hint(Platform::Instagram);
        assert!(
            webm.contains("Instagram") && webm.contains("lebih lama"),
            "{webm}"
        );
        assert!(
            VideoFormat::Mov
                .hint(Platform::Facebook)
                .contains("cukup disalin")
        );
        assert!(VideoFormat::Mp4.hint(Platform::YouTube).contains("1080p"));
        assert!(!VideoFormat::Mp4.hint(Platform::Facebook).contains("1080p"));
        assert!(
            AudioFormat::Opus
                .hint(Platform::Facebook)
                .contains("diubah ulang")
        );
        assert!(AudioFormat::Opus.hint(Platform::YouTube).contains("asli"));
    }

    // ----- Uji pemilihan format dengan yt-dlp sungguhan, tanpa internet -----
    //
    // yt-dlp membaca info-json tiruan ala YouTube (`--load-info-json`) dan hanya mensimulasikan
    // pilihannya (`--simulate`). Jalankan dengan, misalnya:
    // `DANLOAD_TEST_YTDLP=... DANLOAD_TEST_FFMPEG_DIR=... cargo test real_yt_dlp_selects -- --ignored`

    /// Satu format video tiruan. `res` = sisi terpendek gambar.
    fn fake_video(codec: &str, width: u32, height: u32, hdr: bool) -> serde_json::Value {
        let (vcodec, ext) = match (codec, hdr) {
            ("avc", _) => ("avc1.640028", "mp4"),
            ("vp9", false) => ("vp9", "webm"),
            ("vp9", true) => ("vp09.02.51.10.01.09.16.09.00", "webm"),
            (_, false) => ("av01.0.08M.08", "mp4"),
            (_, true) => ("av01.0.12M.10.0.110.09.16.09.0", "mp4"),
        };
        let res = width.min(height);
        serde_json::json!({
            "format_id": format!("{codec}{}-{res}", if hdr { "hdr" } else { "" }),
            "url": "http://127.0.0.1:1/x", "protocol": "https", "ext": ext,
            "width": width, "height": height, "fps": 30, "vcodec": vcodec, "acodec": "none",
            "dynamic_range": if hdr { "HDR10" } else { "SDR" },
            "quality": res, "source_preference": -1, "tbr": 1000 + res,
        })
    }

    /// Info-json tiruan: video dengan `codecs` sampai resolusi `top` (H.264 paling tinggi
    /// 1080p, seperti YouTube), ditambah audio AAC (140) dan Opus (251).
    fn fake_info(id: &str, codecs: &[&str], top: u32, vertical: bool, hdr: bool) -> String {
        let mut formats = Vec::new();
        for res in [360u32, 720, 1080, 1440, 2160] {
            let (short, long) = (res, res * 16 / 9);
            let (width, height) = if vertical {
                (short, long)
            } else {
                (long, short)
            };
            for codec in codecs {
                if res <= top && (*codec != "avc" || res <= 1080) {
                    formats.push(fake_video(codec, width, height, false));
                    if hdr && *codec != "avc" {
                        formats.push(fake_video(codec, width, height, true));
                    }
                }
            }
        }
        for (format_id, acodec, ext, abr) in [
            ("140", "mp4a.40.2", "m4a", 129),
            ("251", "opus", "webm", 135),
        ] {
            formats.push(serde_json::json!({
                "format_id": format_id, "url": "http://127.0.0.1:1/a", "protocol": "https",
                "ext": ext, "vcodec": "none", "acodec": acodec, "abr": abr, "tbr": abr,
                "audio_channels": 2, "quality": 3, "source_preference": -1,
            }));
        }
        serde_json::json!({
            "id": id, "title": format!("Uji {id}"), "extractor": "youtube",
            "extractor_key": "Youtube", "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
            "duration": 10, "_type": "video", "formats": formats,
            // Sama dengan urutan bawaan ekstraktor YouTube di yt-dlp 2026.10.26.
            "_format_sort_fields": ["quality", "res", "fps", "hdr:12", "source", "vcodec",
                                    "channels", "acodec", "lang", "proto"],
        })
        .to_string()
    }

    #[test]
    #[ignore = "butuh yt-dlp asli (DANLOAD_TEST_YTDLP) dan FFmpeg (DANLOAD_TEST_FFMPEG_DIR)"]
    fn real_yt_dlp_selects_expected_formats() {
        let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} belum diisi"));
        let (yt_dlp, ffmpeg_dir) = (env("DANLOAD_TEST_YTDLP"), env("DANLOAD_TEST_FFMPEG_DIR"));
        let dir = std::env::temp_dir().join(format!("danload-pilihan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cases = [
            (
                "lengkap",
                fake_info("lengkap", &["avc", "vp9", "av1"], 2160, false, false),
            ),
            (
                "hdr",
                fake_info("hdr", &["avc", "vp9", "av1"], 2160, false, true),
            ),
            (
                "shorts",
                fake_info("shorts", &["avc", "vp9", "av1"], 1080, true, false),
            ),
            (
                "tanpa-avc",
                fake_info("tanpa-avc", &["vp9", "av1"], 2160, false, false),
            ),
            (
                "avc-saja",
                fake_info("avc-saja", &["avc"], 1080, false, false),
            ),
        ];
        for (name, json) in &cases {
            std::fs::write(dir.join(format!("{name}.json")), json).unwrap();
        }
        let select = |format: OutputFormat, case: &str| -> String {
            let output = std::process::Command::new(&yt_dlp)
                .args(["--ignore-config", "--no-warnings", "--simulate"])
                .args(["--ffmpeg-location", &ffmpeg_dir])
                .arg("--load-info-json")
                .arg(dir.join(format!("{case}.json")))
                .args(["--print", "%(format_id)s → %(ext)s"])
                .args(format.args())
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        let expected = [
            // MP4: resolusi tetap utama; di resolusi yang sama H.264, SDR, dan AAC.
            (
                video(VideoFormat::Mp4, None),
                "lengkap",
                "vp9-2160+140 → mp4",
            ),
            (
                video(VideoFormat::Mp4, Some(1080)),
                "lengkap",
                "avc-1080+140 → mp4",
            ),
            (
                video(VideoFormat::Mp4, Some(1080)),
                "hdr",
                "avc-1080+140 → mp4",
            ),
            (video(VideoFormat::Mp4, None), "hdr", "vp9-2160+140 → mp4"),
            (
                video(VideoFormat::Mp4, None),
                "shorts",
                "avc-1080+140 → mp4",
            ),
            (
                video(VideoFormat::Mp4, Some(720)),
                "tanpa-avc",
                "vp9-720+140 → mp4",
            ),
            // MKV: kualitas terbaik apa adanya (urutan bawaan yt-dlp).
            (
                video(VideoFormat::Mkv, None),
                "lengkap",
                "av1-2160+251 → mkv",
            ),
            (
                video(VideoFormat::Mkv, None),
                "hdr",
                "av1hdr-2160+251 → mkv",
            ),
            // WEBM: VP9 + Opus langsung ke WEBM; jika hanya ada H.264, lewat MKV.
            (
                video(VideoFormat::Webm, None),
                "lengkap",
                "vp9-2160+251 → webm",
            ),
            (video(VideoFormat::Webm, None), "hdr", "vp9-2160+251 → webm"),
            (
                video(VideoFormat::Webm, None),
                "avc-saja",
                "avc-1080+251 → mkv",
            ),
            // MOV: H.264 + AAC lewat MP4 (disalin DANload); tanpa H.264, lewat MKV.
            (
                video(VideoFormat::Mov, None),
                "lengkap",
                "avc-1080+140 → mp4",
            ),
            (
                video(VideoFormat::Mov, None),
                "tanpa-avc",
                "vp9-2160+140 → mkv",
            ),
            // AVI/MPEG: paling tinggi 1080p, sumber H.264.
            (
                video(VideoFormat::Avi, None),
                "lengkap",
                "avc-1080+140 → mkv",
            ),
            (
                video(VideoFormat::Mpeg, Some(720)),
                "lengkap",
                "avc-720+140 → mkv",
            ),
            (
                video(VideoFormat::Avi, None),
                "tanpa-avc",
                "vp9-1080+140 → mkv",
            ),
            // Audio.
            (audio(AudioFormat::Mp3, 192), "lengkap", "251 → webm"),
            (audio(AudioFormat::M4a, 192), "lengkap", "140 → m4a"),
            (audio(AudioFormat::Opus, 192), "lengkap", "251 → webm"),
            (audio(AudioFormat::Flac, 192), "lengkap", "251 → webm"),
        ];
        let mut wrong = Vec::new();
        for (format, case, want) in expected {
            let got = select(format, case);
            println!("{:<18} {case:<10} {got}", format.label());
            if got != want {
                wrong.push(format!(
                    "{} / {case}: dapat `{got}`, harusnya `{want}`",
                    format.label()
                ));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
}
