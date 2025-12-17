use imap_proto::parser::parse_response;
use imap_proto::types::{MailboxDatum, Response};

#[test]
fn search_response_allows_modseq_modifier() {
    let (rest, resp) = parse_response(b"* SEARCH 53999 (MODSEQ 9387530)\r\n").unwrap();
    assert!(rest.is_empty());

    match resp {
        Response::MailboxData(MailboxDatum::Search(ids)) => assert_eq!(ids, vec![53999]),
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn search_response_allows_modseq_modifier_without_ids() {
    let (rest, resp) = parse_response(b"* SEARCH (MODSEQ 123)\r\n").unwrap();
    assert!(rest.is_empty());

    match resp {
        Response::MailboxData(MailboxDatum::Search(ids)) => assert!(ids.is_empty()),
        other => panic!("unexpected response: {other:?}"),
    }
}
