//! A picture on its way into a message.
//!
//! A terminal cannot receive an image, so there are exactly two doors: the
//! clipboard, which the OS is asked about directly (the terminal is not in that
//! conversation), and a path, which a drag into the window types as text and
//! which is then read off disk.
//!
//! The wire wants `{"name", "b64"}` -- see `main.py`'s `chat` action and
//! `handler/agent/providers/base.py`'s `attachment_note`, which is what tells the
//! model what the picture was called. Both frontends and the worker agree on
//! that shape, so it is built here once.
//!
//! Ported from the Go frontends this replaced: the same size cap, the same
//! magic-byte sniff, the same three clipboard classes. A file the terminal
//! refuses and the window accepts would be a bug report nobody can reproduce.

use std::path::Path;
use std::process::Command;

/// The largest picture accepted, decoded.
///
/// Not the wire's limit -- the worker's stdin takes any length -- but the round
/// trip's: an 8MB photo is ~11MB of base64 re-sent on every round of the turn,
/// and a model reads a 1024px picture just as well.
pub const MAX_IMAGE: usize = 8 << 20;

/// One picture waiting to be sent: what it is called, and its bytes base64'd,
/// which is the form the wire and every provider both want.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Image {
    pub name: String,
    pub b64: String,
}

impl Image {
    /// What the worker is handed. The name is what reaches the model, so the
    /// label the message uses is put in front of the file's own name: `[image 1]`
    /// in the text has to be the picture the model is looking at, and a frontend
    /// numbering them and the model numbering them differently is the one failure
    /// this feature cannot have.
    pub fn json(&self, ordinal: usize) -> serde_json::Value {
        serde_json::json!({ "name": format!("image {ordinal}: {}", self.name), "b64": self.b64 })
    }
}

/// The image kinds any provider takes, by the magic bytes at the head of the
/// file rather than by the extension: a `.png` that is really a PDF is a 400
/// from the provider, several seconds later, blamed on nothing in particular.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    let starts = |m: &[u8]| bytes.len() >= m.len() && &bytes[..m.len()] == m;
    if starts(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if starts(b"\xff\xd8\xff") {
        Some("jpg")
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && starts(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Whether a path is worth opening at all. The cheap half of the test: this runs
/// on a keystroke, so the extension is checked before anything touches the disk.
pub fn looks_like_image(path: &str) -> bool {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp")
}

/// Bytes into an `Image`, or the reason they are not one.
pub fn from_bytes(name: &str, bytes: &[u8]) -> Result<Image, String> {
    if sniff(bytes).is_none() {
        return Err(format!("{name} is not a png, jpeg, gif or webp"));
    }
    if bytes.len() > MAX_IMAGE {
        return Err(format!("{name} is {} - the limit is {}", size(bytes.len()), size(MAX_IMAGE)));
    }
    Ok(Image { name: name.to_string(), b64: base64(bytes) })
}

/// A file off disk. The only place one becomes an `Image`, so the cap and the
/// sniff apply however the path arrived.
pub fn read_file(path: &str) -> Result<Image, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if meta.is_dir() {
        return Err(format!("{path} is a directory"));
    }
    if meta.len() as usize > MAX_IMAGE {
        return Err(format!(
            "{} is {} - the limit is {}",
            path,
            size(meta.len() as usize),
            size(MAX_IMAGE)
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let name = Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("image");
    from_bytes(name, &bytes)
}

/// The picture the system clipboard is holding.
///
/// Not through the terminal: OSC 52 carries text and most terminals will not
/// answer a read of it at all, and the terminal's own paste is *its* idea of the
/// clipboard, which for an image is nothing. The clipboard is the OS's, so the
/// OS is asked.
///
/// A file copied in a file manager counts: it is the same gesture, and the name
/// it comes back with beats the one an anonymous bitmap would get.
pub fn clipboard() -> Result<Image, String> {
    #[cfg(target_os = "macos")]
    let found = macos_clipboard();
    #[cfg(target_os = "windows")]
    let found = windows_clipboard();
    #[cfg(all(unix, not(target_os = "macos")))]
    let found = unix_clipboard();
    found.ok_or_else(|| "no image on the clipboard".to_string())
}

/// AppleScript because the pasteboard is: `pbpaste` deals in text and has no
/// flag that hands over a PNG. Three classes, tried in the order that keeps the
/// most information: a copied file keeps its name, a PNG needs no conversion,
/// and TIFF is what Preview and older apps put there and has to be turned into
/// something a provider accepts.
#[cfg(target_os = "macos")]
fn macos_clipboard() -> Option<Image> {
    const SCRIPT: &str = r#"on run argv
	set outPath to item 1 of argv
	try
		return "file:" & (POSIX path of (the clipboard as «class furl»))
	end try
	try
		set d to the clipboard as «class PNGf»
		my writeTo(d, outPath)
		return "png"
	end try
	try
		set d to the clipboard as «class TIFF»
		my writeTo(d, outPath)
		return "tiff"
	end try
	return "none"
end run

on writeTo(d, outPath)
	set fh to open for access (POSIX file outPath) with write permission
	set eof fh to 0
	write d to fh
	close access fh
end writeTo"#;

    let tmp = std::env::temp_dir().join(format!("cuacode-clip-{}", std::process::id()));
    let mut child = Command::new("osascript")
        .arg("-")
        .arg(&tmp)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    {
        use std::io::Write;
        let mut input = child.stdin.take()?;
        input.write_all(SCRIPT.as_bytes()).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    let verdict = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // The temp file is read before it is removed: every branch below that uses
    // it does so first, and the removal is the last thing the call does.
    let image = match verdict.as_str() {
        v if v.starts_with("file:") => {
            let path = &v["file:".len()..];
            looks_like_image(path).then(|| read_file(path).ok()).flatten()
        }
        // Nothing copied off a screen has a name, and the temp file's own is a
        // worse answer than admitting that.
        "png" => std::fs::read(&tmp).ok().and_then(|b| from_bytes("clipboard.png", &b).ok()),
        "tiff" => {
            let png = tmp.with_extension("png");
            let converted = Command::new("sips")
                .args(["-s", "format", "png"])
                .arg(&tmp)
                .arg("--out")
                .arg(&png)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            let image = converted
                .then(|| std::fs::read(&png).ok())
                .flatten()
                .and_then(|b| from_bytes("clipboard.png", &b).ok());
            let _ = std::fs::remove_file(&png);
            image
        }
        _ => None,
    };
    let _ = std::fs::remove_file(&tmp);
    image
}

/// Wayland first, because a wayland session can still have an X clipboard behind
/// Xwayland, and the one the user copied into is the compositor's.
#[cfg(all(unix, not(target_os = "macos")))]
fn unix_clipboard() -> Option<Image> {
    let tries: [(&str, &[&str]); 3] = [
        ("wl-paste", &["--no-newline", "--type", "image/png"]),
        ("xclip", &["-selection", "clipboard", "-t", "image/png", "-o"]),
        ("xsel", &["--clipboard", "--output"]),
    ];
    for (bin, args) in tries {
        let Ok(out) = Command::new(bin).args(args).output() else { continue };
        if out.status.success() && !out.stdout.is_empty() {
            if let Ok(image) = from_bytes("clipboard.png", &out.stdout) {
                return Some(image);
            }
        }
    }
    None
}

/// STA because the Windows clipboard API refuses to be read from any other
/// apartment, and fails by returning nothing rather than by saying so.
#[cfg(target_os = "windows")]
fn windows_clipboard() -> Option<Image> {
    const SCRIPT: &str = r#"Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$files = [Windows.Forms.Clipboard]::GetFileDropList()
if ($files.Count -gt 0) { "file:" + $files[0]; exit }
$img = [Windows.Forms.Clipboard]::GetImage()
if ($img) { $img.Save($args[0], [System.Drawing.Imaging.ImageFormat]::Png); "png"; exit }
"none""#;
    let tmp = std::env::temp_dir().join(format!("cuacode-clip-{}.png", std::process::id()));
    let out = Command::new("powershell")
        .args(["-NoProfile", "-STA", "-Command", SCRIPT])
        .arg(&tmp)
        .output()
        .ok()?;
    let verdict = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let image = match verdict.as_str() {
        v if v.starts_with("file:") => {
            let path = &v["file:".len()..];
            looks_like_image(path).then(|| read_file(path).ok()).flatten()
        }
        "png" => std::fs::read(&tmp).ok().and_then(|b| from_bytes("clipboard.png", &b).ok()),
        _ => None,
    };
    let _ = std::fs::remove_file(&tmp);
    image
}

/// `1.4MB`, `812KB`, `96B`: a size a person reads rather than counts.
pub fn size(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1}MB", n as f64 / (1u64 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{}KB", n >> 10)
    } else {
        format!("{n}B")
    }
}

/// Standard base64, by hand.
///
/// Four lines of arithmetic against a dependency this crate does not have.
/// `crossterm`'s own base64 is behind its `osc52` feature and is a private
/// dependency of that, and the workspace does not otherwise need one: a picture
/// is the only thing here that is not JSON-shaped, and it is encoded once.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend_from_slice(b"payload");
        v
    }

    #[test]
    fn base64_matches_a_known_encoding() {
        // The one property that matters is that the worker can decode it, and
        // this is the standard's own example set.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_png_is_an_image_and_a_pdf_is_not() {
        let image = from_bytes("shot.png", &png()).expect("a png is an image");
        assert_eq!(image.name, "shot.png");
        assert_eq!(image.b64, base64(&png()));
        assert!(from_bytes("lie.png", b"%PDF-1.7").is_err(), "the extension is not the test");
    }

    #[test]
    fn the_name_the_model_reads_carries_the_number_the_message_uses() {
        // The whole point of the feature: `[image 2]` in the text has to be the
        // second picture the model was handed.
        let image = Image { name: "shot.png".into(), b64: "x".into() };
        let json = image.json(2);
        assert_eq!(json["name"], "image 2: shot.png");
        assert_eq!(json["b64"], "x");
    }

    #[test]
    fn a_path_is_judged_before_it_is_opened() {
        assert!(looks_like_image("/tmp/a.PNG"));
        assert!(looks_like_image("shot.jpeg"));
        assert!(!looks_like_image("/tmp/a.txt"));
        assert!(!looks_like_image("noextension"));
    }

    #[test]
    fn a_file_too_big_is_refused_by_size_not_by_provider() {
        let big = vec![0u8; MAX_IMAGE + 1];
        let mut bytes = png();
        bytes.extend_from_slice(&big);
        let err = from_bytes("huge.png", &bytes).expect_err("over the cap");
        assert!(err.contains("limit"), "{err}");
    }

    #[test]
    fn sizes_read_as_sizes() {
        assert_eq!(size(96), "96B");
        assert_eq!(size(2048), "2KB");
        assert_eq!(size(3 << 20), "3.0MB");
    }
}
