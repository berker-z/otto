use std::fs;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tmp/messari.eml"));

    let raw = fs::read(&path)?;
    let parsed = mailparse::parse_mail(&raw)?;

    let mut plain: Option<Vec<u8>> = None;
    let mut html: Option<Vec<u8>> = None;

    for part in parsed.subparts.iter() {
        let ctype = part.ctype.mimetype.to_lowercase();
        if ctype == "text/plain" && plain.is_none() {
            plain = part.get_body_raw().ok();
        }
        if ctype == "text/html" && html.is_none() {
            html = part.get_body_raw().ok();
        }
    }

    fs::create_dir_all("tmp")?;
    if let Some(body) = plain {
        fs::write("tmp/messaritext.txt", body)?;
    }
    if let Some(body) = html {
        fs::write("tmp/messarihtml.html", body)?;
    }

    Ok(())
}
