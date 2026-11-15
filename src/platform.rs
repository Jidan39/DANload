//! Platform sumber unduhan (YouTube, Facebook, Instagram) dan pengecekan URL.
//!
//! Pengguna memilih platform di bar atas sebelum mengunduh (bawaan: YouTube).
//! URL yang ditempel harus berasal dari platform yang dipilih, dan harus link video, postingan,
//! atau (sejak Fase 7) playlist YouTube. Channel, profil, dan story ditolak; lihat [`LinkProblem`].
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

use serde::{Deserialize, Serialize};

/// Platform yang didukung, sesuai urutan tombol di bar atas.
/// Di `library.json` ditulis huruf kecil, mis. `"youtube"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[default]
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

/// Link dari platform yang benar, tetapi bukan link satu video/postingan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkProblem {
    /// Daftar video sementara YouTube (`watch_videos?video_ids=...`).
    VideoList,
    /// Channel YouTube.
    Channel,
    /// Profil Instagram.
    Profile,
    /// Halaman tag/explore Instagram.
    Tag,
    /// Story Instagram (selalu butuh login).
    Story,
    /// Halaman lain yang bukan video, mis. beranda atau hasil pencarian.
    NotAVideo,
}

impl LinkProblem {
    /// Pesan untuk pengguna; `platform` = platform pemilik link.
    fn message(self, platform: Platform) -> String {
        match self {
            LinkProblem::VideoList => "Link daftar video sementara (watch_videos) belum didukung. Tempel link playlist (youtube.com/playlist?list=...) atau link satu video.".to_string(),
            LinkProblem::Channel => "Ini link channel. Unduh semua video channel belum tersedia; buka salah satu video atau playlist-nya, lalu salin link-nya.".to_string(),
            LinkProblem::Profile => "Ini link profil Instagram. Tempel link postingan atau reel (instagram.com/p/... atau instagram.com/reel/...).".to_string(),
            LinkProblem::Tag => "Ini link tag/explore Instagram. Tempel link postingan atau reel.".to_string(),
            LinkProblem::Story => "Story Instagram butuh login, jadi belum bisa diunduh. Tempel link postingan atau reel.".to_string(),
            LinkProblem::NotAVideo => format!(
                "Ini bukan link video {}. Buka video atau postingannya, lalu salin link-nya.",
                platform.name()
            ),
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
    /// Link playlist YouTube: isinya dibaca dulu, lalu pengguna memilih videonya.
    Playlist,
    /// URL dari platform yang dipilih, tetapi bukan link satu video/postingan.
    NotAVideoLink(LinkProblem),
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
        matches!(self, UrlCheck::Matches | UrlCheck::Playlist)
    }

    /// Pesan untuk ditampilkan di bawah kolom URL (kosong = tidak perlu pesan).
    pub fn message(self, selected: Platform) -> String {
        let chosen = selected.name();
        match self {
            UrlCheck::Empty => String::new(),
            UrlCheck::Matches => format!("Link {chosen} siap diunduh."),
            UrlCheck::Playlist => "Link playlist. Tekan Unduh untuk memilih video.".to_string(),
            UrlCheck::NotAVideoLink(problem) => problem.message(selected),
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
pub fn feedback(selected: Platform, url: &str) -> (bool, String) {
    let result = check(selected, url);
    (result.is_ok(), result.message(selected))
}

/// Mengecek apakah `url` berasal dari platform `selected` dan merupakan link satu
/// video/postingan.
pub fn check(selected: Platform, url: &str) -> UrlCheck {
    let url = url.trim();
    if url.is_empty() {
        return UrlCheck::Empty;
    }
    let Some(parts) = split_url(url) else {
        return UrlCheck::NotAUrl;
    };
    match detect(&parts.host) {
        Some(Platform::YouTube) if selected == Platform::YouTube && is_playlist_page(&parts) => {
            UrlCheck::Playlist
        }
        Some(platform) if platform == selected => match link_problem(platform, &parts) {
            Some(problem) => UrlCheck::NotAVideoLink(problem),
            None => UrlCheck::Matches,
        },
        Some(platform) => UrlCheck::OtherPlatform(platform),
        None => UrlCheck::Unsupported,
    }
}

/// `true` jika `url` adalah link satu video YouTube yang juga menyebut playlist-nya, mis.
/// `watch?v=ID&list=PL...` atau `youtu.be/ID?list=PL...`. Pengguna bisa memilih mengunduh
/// videonya saja (bawaan) atau seluruh playlist.
pub fn video_in_playlist(url: &str) -> bool {
    let Some(parts) = split_url(url.trim()) else {
        return false;
    };
    detect(&parts.host) == Some(Platform::YouTube)
        && parts.has_param("list")
        && link_problem(Platform::YouTube, &parts).is_none()
        && !is_playlist_page(&parts)
}

/// Link halaman playlist YouTube: `playlist?list=...`, atau `watch?list=...` tanpa video.
fn is_playlist_page(url: &UrlParts) -> bool {
    let first = url.segments.first().map(String::as_str);
    url.has_param("list")
        && !url.host.ends_with("youtu.be")
        && match first {
            Some("playlist") => true,
            Some("watch") => !url.has_param("v") && url.segments.len() == 1,
            _ => false,
        }
}

/// Bagian URL yang dipakai untuk pengecekan.
struct UrlParts {
    /// Nama host, huruf kecil.
    host: String,
    /// Bagian path, huruf kecil, tanpa garis miring di awal/akhir, mis. `["reel", "abc"]`.
    segments: Vec<String>,
    /// Query tanpa `?`, huruf kecil, mis. `v=abc&t=10`.
    query: String,
}

impl UrlParts {
    /// `true` jika query punya parameter `name` yang tidak kosong, mis. `list` di `?list=PL1`.
    fn has_param(&self, name: &str) -> bool {
        self.query
            .split('&')
            .any(|pair| match pair.split_once('=') {
                Some((key, value)) => key == name && !value.is_empty(),
                None => false,
            })
    }
}

/// Mencari tahu apakah link dari `platform` bukan link satu video/postingan.
/// `None` berarti link itu boleh diunduh.
///
/// YouTube dan Instagram memakai daftar jenis link yang dikenal: link lain (playlist, channel,
/// profil) ditolak di sini, karena yt-dlp akan mengunduh banyak video sekaligus atau gagal.
/// Bentuk link video Facebook terlalu beragam, jadi hanya beranda yang ditolak.
fn link_problem(platform: Platform, url: &UrlParts) -> Option<LinkProblem> {
    let segments: Vec<&str> = url.segments.iter().map(String::as_str).collect();
    let has_id = |id: Option<&&str>| id.is_some_and(|id| !id.is_empty());
    match platform {
        Platform::YouTube => {
            if url.host == "youtu.be" || url.host.ends_with(".youtu.be") {
                return (!has_id(segments.first())).then_some(LinkProblem::NotAVideo);
            }
            match segments.as_slice() {
                // `watch?v=ID` (boleh ada `&list=...`: bawaannya hanya videonya yang diunduh).
                ["watch", ..] if url.has_param("v") || segments.len() > 1 => None,
                ["shorts" | "live" | "embed" | "v" | "e" | "clip", id, ..] if !id.is_empty() => {
                    None
                }
                ["watch_videos", ..] => Some(LinkProblem::VideoList),
                ["channel" | "c" | "user", ..] => Some(LinkProblem::Channel),
                [first, ..] if first.starts_with('@') => Some(LinkProblem::Channel),
                _ => Some(LinkProblem::NotAVideo),
            }
        }
        Platform::Instagram => {
            let is_post = |kind: &str, id: Option<&&str>| {
                matches!(kind, "p" | "reel" | "reels" | "tv")
                    && id.is_some_and(|id| !id.is_empty() && *id != "audio")
            };
            match segments.as_slice() {
                [kind, rest @ ..] if is_post(kind, rest.first()) => None,
                [_user, kind, rest @ ..] if is_post(kind, rest.first()) => None,
                ["share", ..] => None,
                ["stories", ..] => Some(LinkProblem::Story),
                ["explore", ..] => Some(LinkProblem::Tag),
                [_user] | [_user, "reels" | "tagged"] => Some(LinkProblem::Profile),
                _ => Some(LinkProblem::NotAVideo),
            }
        }
        Platform::Facebook => segments.is_empty().then_some(LinkProblem::NotAVideo),
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

/// Memecah URL menjadi host, path, dan query (semuanya huruf kecil). `None` jika teksnya
/// bukan URL.
///
/// Awalan `http://` atau `https://` boleh tidak ada, karena pengguna sering
/// menyalin link tanpa awalan, mis. `youtu.be/abc`.
fn split_url(url: &str) -> Option<UrlParts> {
    let lower = url.to_lowercase();
    let rest = match lower.split_once("://") {
        Some(("http" | "https", rest)) => rest,
        Some(_) => return None, // Skema lain, mis. `ftp://` atau `file://`.
        None => lower.as_str(),
    };
    // Host berakhir di garis miring, tanda tanya, atau tanda pagar pertama.
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, after) = rest.split_at(host_end);
    // Buang info login (`nama@host`) dan nomor port (`host:8080`).
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?.trim_end_matches('.');
    let valid = host.contains('.')
        && !host.starts_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if !valid {
        return None;
    }

    let after = after.split('#').next().unwrap_or_default();
    let (path, query) = after.split_once('?').unwrap_or((after, ""));
    Some(UrlParts {
        host: host.to_string(),
        segments: path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect(),
        query: query.to_string(),
    })
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
            "https://www.youtube.com/watch?v=abc&list=PL123&index=2",
            "https://www.youtube.com/live/abc?feature=share",
            "https://www.youtube-nocookie.com/embed/abc",
            "http://YOUTUBE.com/WATCH?V=abc",
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
            "https://www.facebook.com/reel/1234567890",
            "https://www.facebook.com/share/v/1AbCdEf/",
        ] {
            assert_eq!(check(Platform::Facebook, url), UrlCheck::Matches, "{url}");
        }
        for url in [
            "https://www.instagram.com/reel/C0abc/",
            "https://instagram.com/p/C0abc/?igsh=xyz",
            "instagr.am/p/abc",
            "https://www.instagram.com/reels/C0abc/",
            "https://www.instagram.com/tv/C0abc",
            "https://www.instagram.com/nama_akun/p/C0abc/",
            "https://www.instagram.com/share/reel/BAabc/",
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
    fn all_three_platforms_can_be_downloaded() {
        assert!(feedback(Platform::YouTube, "https://youtu.be/abc").0);
        assert!(feedback(Platform::Facebook, "https://fb.watch/abc").0);
        assert!(feedback(Platform::Instagram, "https://www.instagram.com/reel/abc/").0);
        let (ok, message) = feedback(Platform::YouTube, "https://fb.watch/abc");
        assert!(!ok);
        assert!(message.contains("Pilih Facebook"), "{message}");
    }

    #[test]
    fn links_that_are_not_one_video_are_explained() {
        let cases = [
            (
                Platform::YouTube,
                "https://www.youtube.com/watch_videos?video_ids=a,b",
                LinkProblem::VideoList,
            ),
            (
                Platform::YouTube,
                "https://www.youtube.com/playlist",
                LinkProblem::NotAVideo,
            ),
            (
                Platform::YouTube,
                "https://www.youtube.com/@NamaChannel",
                LinkProblem::Channel,
            ),
            (
                Platform::YouTube,
                "https://www.youtube.com/@NamaChannel/videos",
                LinkProblem::Channel,
            ),
            (
                Platform::YouTube,
                "https://www.youtube.com/channel/UC123",
                LinkProblem::Channel,
            ),
            (
                Platform::YouTube,
                "https://www.youtube.com/",
                LinkProblem::NotAVideo,
            ),
            (
                Platform::YouTube,
                "https://www.youtube.com/results?search_query=kucing",
                LinkProblem::NotAVideo,
            ),
            (
                Platform::YouTube,
                "https://www.youtube.com/shorts/",
                LinkProblem::NotAVideo,
            ),
            (
                Platform::YouTube,
                "https://youtu.be/",
                LinkProblem::NotAVideo,
            ),
            (
                Platform::Instagram,
                "https://www.instagram.com/nama_akun/",
                LinkProblem::Profile,
            ),
            (
                Platform::Instagram,
                "https://www.instagram.com/nama_akun/reels/",
                LinkProblem::Profile,
            ),
            (
                Platform::Instagram,
                "https://www.instagram.com/stories/nama_akun/123/",
                LinkProblem::Story,
            ),
            (
                Platform::Instagram,
                "https://www.instagram.com/explore/tags/kucing/",
                LinkProblem::Tag,
            ),
            (
                Platform::Instagram,
                "https://www.instagram.com/reels/audio/123/",
                LinkProblem::NotAVideo,
            ),
            (
                Platform::Instagram,
                "https://www.instagram.com/",
                LinkProblem::NotAVideo,
            ),
            (
                Platform::Facebook,
                "https://www.facebook.com/",
                LinkProblem::NotAVideo,
            ),
        ];
        for (platform, url, problem) in cases {
            assert_eq!(
                check(platform, url),
                UrlCheck::NotAVideoLink(problem),
                "{url}"
            );
            let (ok, message) = feedback(platform, url);
            assert!(!ok, "{url}");
            assert!(!message.is_empty(), "{url}");
        }
        assert!(
            feedback(Platform::YouTube, "https://www.youtube.com/@a")
                .1
                .contains("playlist")
        );
        assert!(
            feedback(Platform::Instagram, "https://www.instagram.com/stories/a/1")
                .1
                .contains("login")
        );
        assert!(
            feedback(Platform::Facebook, "https://www.facebook.com/")
                .1
                .contains("Facebook")
        );
    }

    #[test]
    fn youtube_playlists_are_recognized() {
        for url in [
            "https://www.youtube.com/playlist?list=PLabcDEF123",
            "https://music.youtube.com/playlist?list=OLAK5uy_abc",
            "www.youtube.com/watch?list=PL123",
            "https://m.youtube.com/playlist?list=PL1&si=xyz",
        ] {
            assert_eq!(check(Platform::YouTube, url), UrlCheck::Playlist, "{url}");
            assert!(feedback(Platform::YouTube, url).0, "{url}");
            assert!(!video_in_playlist(url), "{url}");
        }
        // Playlist hanya untuk YouTube; di platform lain tetap dicek seperti biasa.
        assert_eq!(
            check(
                Platform::Facebook,
                "https://www.youtube.com/playlist?list=PL1"
            ),
            UrlCheck::OtherPlatform(Platform::YouTube)
        );
        // Video yang juga menyebut playlist-nya: tetap link video, dengan pilihan playlist.
        for url in [
            "https://www.youtube.com/watch?v=abc&list=PL123&index=2",
            "https://youtu.be/abc?list=PL123",
        ] {
            assert_eq!(check(Platform::YouTube, url), UrlCheck::Matches, "{url}");
            assert!(video_in_playlist(url), "{url}");
        }
        for url in [
            "https://www.youtube.com/watch?v=abc",
            "https://www.youtube.com/watch?v=abc&list=",
            "https://www.instagram.com/p/abc/?list=PL1",
            "bukan link",
        ] {
            assert!(!video_in_playlist(url), "{url}");
        }
    }

    #[test]
    fn messages_name_both_platforms() {
        let message = UrlCheck::OtherPlatform(Platform::Facebook).message(Platform::YouTube);
        assert!(message.contains("Facebook") && message.contains("YouTube"));
        assert_eq!(UrlCheck::Empty.message(Platform::YouTube), "");
    }
}
