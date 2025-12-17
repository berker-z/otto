use crate::types::BodyRecord;
use anyhow::Result;
use html2text::from_read;
use mailparse::DispositionType;
use mailparse::MailHeaderMap;
use mailparse::ParsedMail;
use mailparse::body::Body;
use serde::Serialize;

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
