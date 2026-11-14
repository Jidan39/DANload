//! Platform sumber unduhan (YouTube, Facebook, Instagram) dan pengecekan URL.
//!
//! Pengguna memilih platform di bar atas sebelum mengunduh (bawaan: YouTube).
//! URL yang ditempel harus berasal dari platform yang dipilih.
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

/// Platform yang didukung, sesuai urutan tombol di bar atas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    YouTube,
    Facebook,
    Instagram,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::YouTube, Platform::Facebook, Platform::Instagram];

    /// Nama untuk ditampilkan ke pengguna.
    pub fn name(self) -> &'static str {
        match self {
            Platform::YouTube => "YouTube",
            Platform::Facebook => "Facebook",
            Platform::Instagram => "Instagram",
        }
    }

    /// Platform dari indeks tombol di UI (0 = YouTube, 1 = Facebook, 2 = Instagram).
    /// Indeks yang tidak dikenal dianggap YouTube, platform bawaan.
    pub fn from_index(index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|index| Platform::ALL.get(index).copied())
            .unwrap_or(Platform::YouTube)
    }

    /// `true` jika unduhan dari platform ini sudah didukung.
    /// Fase 3: baru YouTube. Facebook dan Instagram menyusul di Fase 5.
    pub fn download_supported(self) -> bool {
        matches!(self, Platform::YouTube)
    }

    /// Domain milik platform ini. Subdomain ikut cocok
    /// (mis. `m.youtube.com` dan `music.youtube.com` cocok dengan `youtube.com`).
    fn domains(self) -> &'static [&'static str] {
        match self {
            Platform::YouTube => &["youtube.com", "youtu.be", "youtube-nocookie.com"],
            Platform::Facebook => &["facebook.com", "fb.watch", "fb.com"],
            Platform::Instagram => &["instagram.com", "instagr.am"],
        }
    }
}

/// Hasil pengecekan URL terhadap platform yang dipilih.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlCheck {
    /// Kolom URL masih kosong.
    Empty,
    /// URL berasal dari platform yang dipilih: boleh diunduh.
    Matches,
    /// URL berasal dari platform lain yang juga didukung.
    OtherPlatform(Platform),
    /// URL valid, tetapi bukan dari platform yang didukung.
    Unsupported,
    /// Teksnya bukan URL.
    NotAUrl,
}

impl UrlCheck {
    /// `true` jika URL boleh diunduh dengan platform yang sedang dipilih.
    pub fn is_ok(self) -> bool {
        self == UrlCheck::Matches
    }

    /// Pesan untuk ditampilkan di bawah kolom URL (kosong = tidak perlu pesan).
    pub fn message(self, selected: Platform) -> String {
        let chosen = selected.name();
        match self {
            UrlCheck::Empty => String::new(),
            UrlCheck::Matches => format!("Link {chosen} siap diunduh."),
            UrlCheck::OtherPlatform(other) => {
                let other = other.name();
                format!(
                    "Ini link {other}, bukan {chosen}. Pilih {other} di kanan atas untuk mengunduhnya."
                )
            }
            UrlCheck::Unsupported => format!(
                "Link ini bukan dari {chosen}. DANload hanya mendukung YouTube, Facebook, dan Instagram."
            ),
            UrlCheck::NotAUrl => {
                "Ini bukan link yang valid. Salin link lengkapnya dari browser atau aplikasi."
                    .to_string()
            }
        }
    }
}

/// Hasil pengecekan untuk UI: `(boleh_diunduh, pesan_di_bawah_kolom_URL)`.
///
/// Boleh diunduh jika link cocok dengan platform terpilih **dan** platform itu sudah
/// didukung untuk diunduh.
pub fn feedback(selected: Platform, url: &str) -> (bool, String) {
    let result = check(selected, url);
    if result.is_ok() && !selected.download_supported() {
        let name = selected.name();
        return (
            false,
            format!(
                "Link {name} sudah benar, tetapi unduhan {name} baru tersedia di tahap berikutnya. Untuk sekarang baru YouTube."
            ),
        );
    }
    (result.is_ok(), result.message(selected))
}

/// Mengecek apakah `url` berasal dari platform `selected`.
pub fn check(selected: Platform, url: &str) -> UrlCheck {
    let url = url.trim();
    if url.is_empty() {
        return UrlCheck::Empty;
    }
    let Some(host) = host_of(url) else {
        return UrlCheck::NotAUrl;
    };
    match detect(&host) {
        Some(platform) if platform == selected => UrlCheck::Matches,
        Some(platform) => UrlCheck::OtherPlatform(platform),
        None => UrlCheck::Unsupported,
    }
}

/// Platform pemilik sebuah nama host, mis. `www.youtube.com` → YouTube.
fn detect(host: &str) -> Option<Platform> {
    Platform::ALL.into_iter().find(|platform| {
        platform.domains().iter().any(|domain| {
            // Cocok jika sama persis, atau berupa subdomainnya (diawali titik).
            // `notyoutube.com` sengaja tidak dianggap YouTube.
            host == *domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
    })
}

/// Mengambil nama host (huruf kecil) dari sebuah URL. `None` jika teksnya bukan URL.
///
/// Awalan `http://` atau `https://` boleh tidak ada, karena pengguna sering
/// menyalin link tanpa awalan, mis. `youtu.be/abc`.
fn host_of(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let rest = match lower.split_once("://") {
        Some(("http" | "https", rest)) => rest,
        Some(_) => return None, // Skema lain, mis. `ftp://` atau `file://`.
        None => lower.as_str(),
    };
    // Host berakhir di garis miring, tanda tanya, atau tanda pagar pertama.
    let authority = rest.split(['/', '?', '#']).next()?;
    // Buang info login (`nama@host`) dan nomor port (`host:8080`).
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?.trim_end_matches('.');

    let valid = host.contains('.')
        && !host.starts_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then(|| host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_links_match_youtube() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://m.youtube.com/shorts/abc123",
            "https://music.youtube.com/watch?v=abc",
            "http://YOUTUBE.com/playlist?list=PL123",
            "youtu.be/abc",
            "  www.youtube.com/watch?v=abc  ",
            "https://www.youtube.com:443/watch?v=abc",
        ] {
            assert_eq!(check(Platform::YouTube, url), UrlCheck::Matches, "{url}");
        }
    }

    #[test]
    fn facebook_and_instagram_links_match() {
        for url in [
            "https://www.facebook.com/watch/?v=123",
            "https://web.facebook.com/user/videos/123",
            "https://fb.watch/abcDEF/",
            "https://m.facebook.com/story.php?id=1",
        ] {
            assert_eq!(check(Platform::Facebook, url), UrlCheck::Matches, "{url}");
        }
        for url in [
            "https://www.instagram.com/reel/C0abc/",
            "https://instagram.com/p/C0abc/?igsh=xyz",
            "instagr.am/p/abc",
        ] {
            assert_eq!(check(Platform::Instagram, url), UrlCheck::Matches, "{url}");
        }
    }

    #[test]
    fn link_from_other_platform_is_reported() {
        assert_eq!(
            check(Platform::YouTube, "https://www.instagram.com/reel/abc/"),
            UrlCheck::OtherPlatform(Platform::Instagram)
        );
        assert_eq!(
            check(Platform::Instagram, "https://youtu.be/abc"),
            UrlCheck::OtherPlatform(Platform::YouTube)
        );
    }

    #[test]
    fn look_alike_domains_are_not_trusted() {
        for url in [
            "https://notyoutube.com/watch?v=abc",
            "https://youtube.com.evil.example/watch",
            "https://www.youtube.co/watch?v=abc",
            "https://evil.example/?u=https://youtube.com",
        ] {
            assert_eq!(
                check(Platform::YouTube, url),
                UrlCheck::Unsupported,
                "{url}"
            );
        }
    }

    #[test]
    fn login_info_cannot_disguise_host() {
        // Host sebenarnya adalah evil.example, bukan youtube.com.
        assert_eq!(
            check(Platform::YouTube, "https://youtube.com@evil.example/watch"),
            UrlCheck::Unsupported
        );
    }

    #[test]
    fn non_urls_are_rejected() {
        for text in [
            "halo",
            "video kucing lucu",
            "ftp://youtube.com/abc",
            "file:///C:/video.mp4",
            "https://",
            "https:///watch",
        ] {
            assert_eq!(check(Platform::YouTube, text), UrlCheck::NotAUrl, "{text}");
        }
        assert_eq!(check(Platform::YouTube, "   "), UrlCheck::Empty);
    }

    #[test]
    fn index_mapping_defaults_to_youtube() {
        assert_eq!(Platform::from_index(0), Platform::YouTube);
        assert_eq!(Platform::from_index(1), Platform::Facebook);
        assert_eq!(Platform::from_index(2), Platform::Instagram);
        assert_eq!(Platform::from_index(-1), Platform::YouTube);
        assert_eq!(Platform::from_index(7), Platform::YouTube);
    }

    #[test]
    fn only_matching_link_is_ok() {
        assert!(check(Platform::YouTube, "https://youtu.be/abc").is_ok());
        assert!(!check(Platform::Facebook, "https://youtu.be/abc").is_ok());
        assert!(!check(Platform::YouTube, "").is_ok());
    }

    #[test]
    fn only_youtube_can_be_downloaded_for_now() {
        assert!(feedback(Platform::YouTube, "https://youtu.be/abc").0);
        let (ok, message) = feedback(Platform::Facebook, "https://fb.watch/abc");
        assert!(!ok);
        assert!(message.contains("tahap berikutnya"), "{message}");
        let (ok, message) = feedback(Platform::YouTube, "https://fb.watch/abc");
        assert!(!ok);
        assert!(message.contains("Pilih Facebook"), "{message}");
    }

    #[test]
    fn messages_name_both_platforms() {
        let message = UrlCheck::OtherPlatform(Platform::Facebook).message(Platform::YouTube);
        assert!(message.contains("Facebook") && message.contains("YouTube"));
        assert_eq!(UrlCheck::Empty.message(Platform::YouTube), "");
    }
}
