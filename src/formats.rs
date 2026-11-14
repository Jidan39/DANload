//! Pilihan format unduhan (Fase 3: MP4 dan MP3) dan argumen yt-dlp untuk tiap format.
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

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

/// Pilihan bitrate MP3 (kbps), sesuai urutan di menu.
pub const AUDIO_BITRATES: [u32; 4] = [128, 192, 256, 320];

/// Bitrate MP3 bawaan, sama dengan aplikasi lama.
pub const DEFAULT_AUDIO_BITRATE: u32 = 192;

/// Format hasil unduhan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Video MP4. `max_height`: batas tinggi gambar (mis. 1080), `None` = terbaik.
    Mp4 { max_height: Option<u32> },
    /// Audio MP3 dengan bitrate tertentu (kbps).
    Mp3 { bitrate_kbps: u32 },
}

impl OutputFormat {
    /// Label singkat untuk ditampilkan, mis. `MP4 · 1080p` atau `MP3 · 192 kbps`.
    pub fn label(self) -> String {
        match self {
            OutputFormat::Mp4 { max_height } => format!("MP4 · {}", height_label(max_height)),
            OutputFormat::Mp3 { bitrate_kbps } => format!("MP3 · {}", bitrate_label(bitrate_kbps)),
        }
    }

    /// Argumen yt-dlp untuk memilih dan membentuk format ini.
    ///
    /// MP4 memakai aturan yang sama dengan aplikasi lama: utamakan video MP4 + audio M4A,
    /// lalu video + audio apa pun, lalu satu file terbaik. Batas tinggi memakai `<=?`
    /// supaya format yang tingginya tidak diketahui tidak ikut tersaring habis.
    pub fn args(self) -> Vec<String> {
        match self {
            OutputFormat::Mp4 { max_height } => {
                let limit = max_height
                    .map(|height| format!("[height<=?{height}]"))
                    .unwrap_or_default();
                vec![
                    "-f".into(),
                    format!(
                        "bestvideo{limit}[ext=mp4]+bestaudio[ext=m4a]/bestvideo{limit}+bestaudio/best{limit}"
                    ),
                    "--merge-output-format".into(),
                    "mp4".into(),
                ]
            }
            OutputFormat::Mp3 { bitrate_kbps } => vec![
                "-f".into(),
                "bestaudio/best".into(),
                "--extract-audio".into(),
                "--audio-format".into(),
                "mp3".into(),
                "--audio-quality".into(),
                format!("{bitrate_kbps}K"),
                "--embed-metadata".into(),
            ],
        }
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

/// Label semua pilihan resolusi, sesuai urutan di menu.
pub fn video_height_labels() -> Vec<String> {
    VIDEO_HEIGHTS.into_iter().map(height_label).collect()
}

/// Label semua pilihan bitrate, sesuai urutan di menu.
pub fn audio_bitrate_labels() -> Vec<String> {
    AUDIO_BITRATES.into_iter().map(bitrate_label).collect()
}

/// Posisi `height` di menu resolusi. Nilai yang tidak ada di menu dianggap "Terbaik".
pub fn video_height_index(height: Option<u32>) -> usize {
    VIDEO_HEIGHTS
        .iter()
        .position(|candidate| *candidate == height)
        .unwrap_or(0)
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

    #[test]
    fn mp4_args_follow_old_app_with_tolerant_limit() {
        let args = OutputFormat::Mp4 {
            max_height: Some(1080),
        }
        .args();
        assert_eq!(
            args,
            vec![
                "-f",
                "bestvideo[height<=?1080][ext=mp4]+bestaudio[ext=m4a]/bestvideo[height<=?1080]+bestaudio/best[height<=?1080]",
                "--merge-output-format",
                "mp4",
            ]
        );
        let best = OutputFormat::Mp4 { max_height: None }.args();
        assert_eq!(
            best[1],
            "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo+bestaudio/best"
        );
    }

    #[test]
    fn mp3_args_set_bitrate_and_metadata() {
        let args = OutputFormat::Mp3 { bitrate_kbps: 320 }.args();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--audio-format", "mp3"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--audio-quality", "320K"])
        );
        assert!(args.contains(&"--extract-audio".to_string()));
        assert!(args.contains(&"--embed-metadata".to_string()));
    }

    #[test]
    fn labels() {
        assert_eq!(
            OutputFormat::Mp4 { max_height: None }.label(),
            "MP4 · Terbaik"
        );
        assert_eq!(
            OutputFormat::Mp4 {
                max_height: Some(720)
            }
            .label(),
            "MP4 · 720p"
        );
        assert_eq!(
            OutputFormat::Mp3 { bitrate_kbps: 192 }.label(),
            "MP3 · 192 kbps"
        );
        assert_eq!(video_height_labels()[0], "Terbaik");
        assert_eq!(audio_bitrate_labels().len(), AUDIO_BITRATES.len());
    }

    #[test]
    fn menu_indexes_fall_back_safely() {
        assert_eq!(video_height_index(None), 0);
        assert_eq!(video_height_index(Some(1080)), 3);
        assert_eq!(video_height_index(Some(999)), 0);
        assert_eq!(audio_bitrate_index(320), 3);
        assert_eq!(audio_bitrate_index(0), 1); // tidak dikenal → 192 kbps
    }
}
