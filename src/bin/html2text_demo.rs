use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::PathBuf;
use std::time::Instant;

use regex::Regex;
use url::Url;
use url::form_urlencoded;

fn main() -> io::Result<()> {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tmp/linkedin_body.html"));

    let file = File::open(&path)?;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;

    let body = std::str::from_utf8(&buf).unwrap_or_default().to_string();
    let cleaned = clean_urls_in_text(&body);

    let start = Instant::now();
    let text = if looks_like_html(&cleaned) {
        html2text::from_read(cleaned.as_bytes(), 80).map_err(io::Error::other)?
    } else {
        cleaned
    };
    let elapsed = start.elapsed();

    println!("=== html2text output ===");
    println!("{}", text);
    println!("=== elapsed: {:?} ===", elapsed);

    Ok(())
}

fn looks_like_html(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    if lower.contains("<html")
        || lower.contains("<body")
        || lower.contains("<div")
        || lower.contains("<span")
        || lower.contains("<p")
        || lower.contains("<table")
        || lower.contains("<br")
        || lower.contains("</")
    {
        return true;
    }

    let angle_count = body.as_bytes().iter().filter(|b| **b == b'<').count();
    angle_count > 5
}

fn clean_urls_in_text(body: &str) -> String {
    // Clean URL query params (tracker-heavy ones) without stripping functional params.
    let url_re = Regex::new(r#"https?://[^\s<>()"']+"#).expect("valid regex");
    url_re
        .replace_all(body, |caps: &regex::Captures| {
            let url = &caps[0];
            clean_url(url)
        })
        .into_owned()
}

fn clean_url(raw: &str) -> String {
    // Exact matches to strip quickly.
    const DROP_EXACT: &[&str] = &[
        "gclid", "dclid", "fbclid", "msclkid", "yclid", "mc_eid", "mc_cid", "mkt_tok", "lipi",
        "loid", "lang",
    ];
    // Prefix-based tracking params (e.g., utm_source, utm_campaign, li_*).
    const DROP_PREFIXES: &[&str] = &[
        "utm_",
        "fbclid",
        "gclid",
        "dclid",
        "msclkid",
        "yclid",
        "mc_",
        "mkt_",
        "trk",
        "trkEmail",
        "mid",
        "li_",
        "eid",
        "cid",
        "ref",
        "spm",
        "sr_",
        "sc_",
        "oly_",
        "campaignId",
        "emailKey",
        "uuid",
    ];

    if let Some(unwrapped) = try_unwrap_redirect(raw) {
        return unwrapped;
    }

    let Ok(mut parsed) = Url::parse(raw) else {
        return raw.to_string();
    };

    let mut kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| {
            let key = k.as_ref();
            if DROP_EXACT.contains(&key) {
                return false;
            }
            !DROP_PREFIXES.iter().any(|p| key.starts_with(p))
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if kept.is_empty() {
        parsed.set_query(None);
        return parsed.to_string();
    }

    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (k, v) in kept.drain(..) {
        serializer.append_pair(&k, &v);
    }
    let new_query = serializer.finish();
    parsed.set_query(Some(&new_query));
    parsed.to_string()
}

fn try_unwrap_redirect(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw).ok()?;
    let host = parsed.host_str().unwrap_or_default();
    let path = parsed.path();
    let query_pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let pick_param = |keys: &[&str]| -> Option<String> {
        for k in keys {
            if let Some((_, v)) = query_pairs.iter().find(|(key, _)| key == k) {
                return Url::parse(v).ok().map(|u| clean_url(u.as_ref()));
            }
        }
        None
    };

    // Outlook/OWA redirect pattern.
    if host.contains("outlook.live.com")
        && path.contains("redir")
        && let Some(dest) = pick_param(&["url", "destination"])
    {
        return Some(dest);
    }

    // LinkedIn shorteners/safety redirects.
    if (host.ends_with("lnkd.in") || host.contains("linkedin.com") && path.contains("redir"))
        && let Some(dest) = pick_param(&["url", "dest", "target"])
    {
        return Some(dest);
    }

    // Generic redirect params.
    if let Some(dest) = pick_param(&["url", "u", "target", "dest", "redirect", "redirect_uri"]) {
        return Some(dest);
    }

    None
}
