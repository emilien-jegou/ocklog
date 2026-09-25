use std::io::Write;
use std::process::{Command, Stdio};

pub struct Clipboard;

impl Clipboard {
    pub fn copy(text: &str) {
        Self::copy_osc52(text);
        #[cfg(target_os = "macos")]
        Self::copy_macos(text);
        #[cfg(target_os = "linux")]
        Self::copy_linux(text);
    }

    fn copy_osc52(text: &str) {
        let encoded = base64_encode(text);
        let osc52 = format!("\x1b]52;c;{}\x07", encoded);
        let mut out = std::io::stdout();
        let _ = out.write_all(osc52.as_bytes());
        let _ = out.flush();
    }

    #[cfg(target_os = "macos")]
    fn copy_macos(text: &str) {
        if let Ok(mut child) = Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn copy_linux(text: &str) {
        let spawned = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .spawn()
            .or_else(|_| Command::new("xclip").arg("-selection").arg("clipboard").stdin(Stdio::piped()).spawn());

        if let Ok(mut child) = spawned {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
        }
    }
}

fn base64_encode(input: &str) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        out.push(CHARSET[(b0 >> 2) as usize] as char);
        out.push(CHARSET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);

        if chunk.len() > 1 {
            out.push(CHARSET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }

        if chunk.len() > 2 {
            out.push(CHARSET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
