use crate::types::BodyRecord;
use anyhow::Result;
use html2text::from_read;
use mailparse::DispositionType;
use mailparse::MailHeaderMap;
use mailparse::ParsedMail;
use mailparse::body::Body;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use url::Url;
use url::form_urlencoded;

#[derive(Debug)]
pub struct SanitizedBody {
    pub sanitized_text: String,
    pub mime_summary: Option<String>,
    pub attachments_json: Option<String>,
    pub raw_hash: String,
    pub has_attachments: bool,
}

#[derive(Debug, Serialize)]
struct AttachmentMeta {
    filename: Option<String>,
    mime_type: String,
    disposition: String,
    content_id: Option<String>,
    encoded_bytes: usize,
}

pub fn sanitize(parsed: &ParsedMail, raw_bytes: &[u8]) -> Result<SanitizedBody> {
    let text = extract_text(parsed, raw_bytes);
    let raw_hash = compute_hash(raw_bytes);
    let (mime_summary, attachments) = summarize_mime(parsed);
    let has_attachments = !attachments.is_empty();

    Ok(SanitizedBody {
        sanitized_text: text,
        mime_summary: Some(mime_summary),
        attachments_json: serde_json::to_string(&attachments).ok(),
        raw_hash,
        has_attachments,
    })
}

/// Public wrapper for sanitize that's imported by sync module
pub fn sanitize_message(parsed: &ParsedMail, raw_bytes: &[u8]) -> SanitizedBody {
    sanitize(parsed, raw_bytes).unwrap_or_else(|_| SanitizedBody {
        sanitized_text: String::from_utf8_lossy(raw_bytes).to_string(),
        mime_summary: None,
        attachments_json: None,
        raw_hash: compute_hash(raw_bytes),
        has_attachments: false,
    })
}

fn compute_hash(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn summarize_mime(parsed: &ParsedMail) -> (String, Vec<AttachmentMeta>) {
    let mut lines = Vec::new();
    let mut attachments = Vec::new();
    walk_mime(parsed, 0, &mut lines, &mut attachments);

    let summary = if lines.is_empty() {
        "(empty MIME)".to_string()
    } else {
        lines.join("\n")
    };

    (summary, attachments)
}

fn walk_mime(
    part: &ParsedMail,
    depth: usize,
    lines: &mut Vec<String>,
    attachments: &mut Vec<AttachmentMeta>,
) {
    // Hard cap to avoid pathological MIME blowing up output.
    if lines.len() > 300 || depth > 20 {
        return;
    }

    let ctype = &part.ctype;
    let disp = part.get_content_disposition();
    let filename = extract_filename(part);
    let content_id = part
        .headers
        .get_first_value("Content-ID")
        .map(|v| v.trim().trim_matches(&['<', '>'][..]).to_string());

    let (disposition, encoded_bytes) = match part.get_body_encoded() {
        Body::Base64(b) => (disp_to_string(&disp.disposition), b.get_raw().len()),
        Body::QuotedPrintable(b) => (disp_to_string(&disp.disposition), b.get_raw().len()),
        Body::SevenBit(b) => (disp_to_string(&disp.disposition), b.get_raw().len()),
        Body::EightBit(b) => (disp_to_string(&disp.disposition), b.get_raw().len()),
        Body::Binary(b) => (disp_to_string(&disp.disposition), b.get_raw().len()),
    };

    let indent = "  ".repeat(depth);
    let mut line = format!("{indent}{}", ctype.mimetype);
    if ctype.mimetype.starts_with("text/") && !ctype.charset.is_empty() {
        line.push_str(&format!("; charset={}", ctype.charset));
    }
    if !disposition.is_empty() {
        line.push_str(&format!("; disp={}", disposition));
    }
    if let Some(ref name) = filename {
        line.push_str(&format!("; filename={}", name));
    }
    if let Some(ref cid) = content_id {
        line.push_str(&format!("; cid={}", cid));
    }
    if encoded_bytes > 0 {
        line.push_str(&format!("; bytes={}", encoded_bytes));
    }
    lines.push(line);

    let is_container = ctype.mimetype.starts_with("multipart/") && !part.subparts.is_empty();
    if !is_container
        && is_attachment_part(
            &ctype.mimetype,
            &disp.disposition,
            filename.as_deref(),
            content_id.as_deref(),
        )
    {
        attachments.push(AttachmentMeta {
            filename,
            mime_type: ctype.mimetype.clone(),
            disposition,
            content_id,
            encoded_bytes,
        });
    }

    for child in &part.subparts {
        walk_mime(child, depth + 1, lines, attachments);
    }
}

fn extract_filename(part: &ParsedMail) -> Option<String> {
    let disp = part.get_content_disposition();
    let disp_name = disp
        .params
        .get("filename")
        .or_else(|| disp.params.get("name"))
        .cloned();

    let ctype_name = part
        .ctype
        .params
        .get("name")
        .or_else(|| part.ctype.params.get("filename"))
        .cloned();

    disp_name.or(ctype_name).and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn disp_to_string(disp: &DispositionType) -> String {
    match disp {
        DispositionType::Inline => "inline".to_string(),
        DispositionType::Attachment => "attachment".to_string(),
        DispositionType::FormData => "form-data".to_string(),
        DispositionType::Extension(v) => v.clone(),
    }
}

fn is_attachment_part(
    mimetype: &str,
    disposition: &DispositionType,
    filename: Option<&str>,
    content_id: Option<&str>,
) -> bool {
    if matches!(disposition, DispositionType::Attachment) {
        return true;
    }
    if filename.is_some() {
        return true;
    }
    if content_id.is_some() && !mimetype.starts_with("text/") {
        return true;
    }
    !mimetype.starts_with("text/") && !mimetype.starts_with("multipart/")
}

fn extract_text(parsed: &ParsedMail, raw_bytes: &[u8]) -> String {
    if let Some(text) = extract_preferred_text(parsed) {
        return text;
    }
    // As last resort, render the whole raw message body.
    render_text_part(&String::from_utf8_lossy(raw_bytes).to_string())
}

fn html_to_text(html: &[u8]) -> String {
    from_read(html, 80).unwrap_or_default()
}

fn render_text_part(body: &str) -> String {
    let cleaned = clean_urls_in_text(body);
    if looks_like_html(&cleaned) {
        html_to_text(cleaned.as_bytes())
    } else {
        cleaned
    }
}

fn render_html_part(html: &[u8]) -> String {
    let cleaned = clean_urls_in_text(&String::from_utf8_lossy(html));
    html_to_text(cleaned.as_bytes())
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

fn extract_preferred_text(part: &ParsedMail) -> Option<String> {
    let mimetype = part.ctype.mimetype.to_ascii_lowercase();
    if part.subparts.is_empty() {
        if mimetype == "text/plain" {
            let body = String::from_utf8_lossy(part.get_body_raw().unwrap_or_default().as_ref())
                .to_string();
            return Some(render_text_part(&body));
        }
        if mimetype == "text/html" {
            let html = part.get_body_raw().unwrap_or_default();
            return Some(render_html_part(&html));
        }
        return None;
    }

    // Handle multipart/alternative with preference: text/plain then text/html then others.
    if mimetype.starts_with("multipart/alternative") {
        if let Some(text_part) = part
            .subparts
            .iter()
            .find(|p| p.ctype.mimetype.eq_ignore_ascii_case("text/plain"))
        {
            if let Some(text) = extract_preferred_text(text_part) {
                return Some(text);
            }
        }
        if let Some(html_part) = part
            .subparts
            .iter()
            .find(|p| p.ctype.mimetype.eq_ignore_ascii_case("text/html"))
        {
            if let Some(text) = extract_preferred_text(html_part) {
                return Some(text);
            }
        }
    }

    // For other multiparts, walk children and return the first successful extraction.
    for child in &part.subparts {
        if let Some(text) = extract_preferred_text(child) {
            return Some(text);
        }
    }

    None
}

fn clean_urls_in_text(body: &str) -> String {
    // Clean URL query params (tracker-heavy ones) without stripping functional params.
    static URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"https?://[^\s<>()"']+"#).unwrap());

    URL_RE
        .replace_all(body, |caps: &regex::Captures| {
            let url = &caps[0];
            clean_url(url)
        })
        .into_owned()
}

fn clean_url(raw: &str) -> String {
    // Exact matches to strip quickly.
    const DROP_EXACT: &[&str] = &[
        "gclid",
        "dclid",
        "fbclid",
        "msclkid",
        "yclid",
        "mc_eid",
        "mc_cid",
        "mkt_tok",
        "lipi",
        "loid",
        "lang",
        "trackingId",
        "trackId",
        "tracking",
        "token",
        "otpToken",
        "sparams",
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
        "tracking",
        "token",
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
                return Url::parse(v).ok().map(|u| clean_url(&u.to_string()));
            }
        }
        None
    };

    // Outlook/OWA redirect pattern.
    if host.contains("outlook.live.com") && path.contains("redir") {
        if let Some(dest) = pick_param(&["url", "destination"]) {
            return Some(dest);
        }
    }

    // LinkedIn shorteners/safety redirects.
    if host.ends_with("lnkd.in") || (host.contains("linkedin.com") && path.contains("redir")) {
        if let Some(dest) = pick_param(&["url", "dest", "target"]) {
            return Some(dest);
        }
    }

    // Generic redirect params.
    if let Some(dest) = pick_param(&["url", "u", "target", "dest", "redirect", "redirect_uri"]) {
        return Some(dest);
    }

    None
}

pub fn build_body_record(
    message_id: &str,
    raw: Option<Vec<u8>>,
    sanitized: SanitizedBody,
) -> BodyRecord {
    BodyRecord {
        message_id: message_id.to_string(),
        raw_rfc822: raw,
        sanitized_text: Some(sanitized.sanitized_text),
        mime_summary: sanitized.mime_summary,
        attachments_json: sanitized.attachments_json,
        sanitized_at: Some(crate::types::now_ts()),
    }
}
