use mailparse::parse_mail;

use otto::sanitize::sanitize_message;

#[test]
fn sanitize_produces_mime_summary_and_attachments_json() {
    let raw = concat!(
        "Subject: test\r\n",
        "Content-Type: multipart/mixed; boundary=\"b\"\r\n",
        "\r\n",
        "--b\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Hello\r\n",
        "--b\r\n",
        "Content-Type: application/pdf; name=\"file.pdf\"\r\n",
        "Content-Disposition: attachment; filename=\"file.pdf\"\r\n",
        "Content-Transfer-Encoding: base64\r\n",
        "\r\n",
        "SGVsbG8=\r\n",
        "--b--\r\n",
    )
    .as_bytes()
    .to_vec();

    let parsed = parse_mail(&raw).expect("parse_mail");
    let sanitized = sanitize_message(&parsed, &raw);

    assert!(sanitized.has_attachments);
    assert!(sanitized
        .mime_summary
        .as_deref()
        .unwrap_or_default()
        .contains("application/pdf"));
    assert!(sanitized
        .attachments_json
        .as_deref()
        .unwrap_or_default()
        .contains("file.pdf"));
}
