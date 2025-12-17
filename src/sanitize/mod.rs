use crate::types::BodyRecord;
use anyhow::Result;
use html2text::from_read;
use mailparse::ParsedMail;

#[derive(Debug)]
pub struct SanitizedBody {
    pub sanitized_text: String,
    pub mime_summary: Option<String>,
    pub attachments_json: Option<String>,
    pub raw_hash: String,
    pub has_attachments: bool,
}

pub fn sanitize(parsed: &ParsedMail, raw_bytes: &[u8]) -> Result<SanitizedBody> {
    let text = extract_text(parsed, raw_bytes);
    let raw_hash = compute_hash(raw_bytes);
    let has_attachments = detect_attachments(parsed);

    Ok(SanitizedBody {
        sanitized_text: text,
        mime_summary: None,
        attachments_json: None,
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

fn detect_attachments(parsed: &ParsedMail) -> bool {
    // Check if any part has Content-Disposition: attachment
    for part in &parsed.subparts {
        if part.get_content_disposition().disposition == mailparse::DispositionType::Attachment {
            return true;
        }
    }
    false
}

fn extract_text(parsed: &ParsedMail, raw_bytes: &[u8]) -> String {
    if parsed.subparts.is_empty() {
        if parsed.ctype.mimetype.eq_ignore_ascii_case("text/plain") {
            return String::from_utf8_lossy(parsed.get_body_raw().unwrap_or_default().as_ref())
                .to_string();
        }
        if parsed.ctype.mimetype.eq_ignore_ascii_case("text/html") {
            let html = parsed.get_body_raw().unwrap_or_default();
            return html_to_text(&html);
        }
    }

    for part in &parsed.subparts {
        if part.ctype.mimetype.eq_ignore_ascii_case("text/plain") {
            return String::from_utf8_lossy(part.get_body_raw().unwrap_or_default().as_ref())
                .to_string();
        }
    }

    // Fallback: pick first part and convert to text if html, else bytes->lossy string.
    if let Some(first) = parsed.subparts.first() {
        if first.ctype.mimetype.eq_ignore_ascii_case("text/html") {
            let html = first.get_body_raw().unwrap_or_default();
            return html_to_text(&html);
        }
        let raw = first.get_body_raw().unwrap_or_default();
        return String::from_utf8_lossy(raw.as_ref()).to_string();
    }

    // As last resort, render the whole raw message body.
    String::from_utf8_lossy(raw_bytes).to_string()
}

fn html_to_text(html: &[u8]) -> String {
    from_read(html, 80).unwrap_or_default()
}

pub fn build_body_record(message_id: &str, raw: Option<Vec<u8>>, sanitized: SanitizedBody) -> BodyRecord {
    BodyRecord {
        message_id: message_id.to_string(),
        raw_rfc822: raw,
        sanitized_text: Some(sanitized.sanitized_text),
        mime_summary: sanitized.mime_summary,
        attachments_json: sanitized.attachments_json,
        sanitized_at: Some(crate::types::now_ts()),
    }
}
