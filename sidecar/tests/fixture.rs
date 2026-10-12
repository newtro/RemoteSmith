//! The shared protocol fixture (docs/ipc-fixtures/v1.jsonl). The fork's endpoint decodes the
//! same file in `vendor/rustdesk/src/remotesmith/tests.rs`, so both ends agree on every sample.

use remotesmith_tray::protocol::*;

const FIXTURE: &str = include_str!("../../docs/ipc-fixtures/v1.jsonl");

#[derive(serde::Deserialize)]
struct Sample {
    to: String,
    valid: bool,
    #[serde(rename = "type")]
    ty: Option<String>,
    note: String,
    line: String,
}

fn wire_type<T: serde::Serialize>(body: &T) -> Option<String> {
    serde_json::to_value(body).unwrap()["type"]
        .as_str()
        .map(str::to_owned)
}

#[test]
fn shared_fixture_decodes_like_the_fork() {
    let samples: Vec<Sample> = FIXTURE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("fixture line"))
        .collect();
    assert!(samples.len() >= 30);
    for s in &samples {
        let got = match s.to.as_str() {
            "server" => decode::<Request>(s.line.as_bytes()).map(|e| {
                let back: Envelope<Request> =
                    decode(encode(e.v, e.id, &e.body).trim_end().as_bytes()).unwrap();
                assert_eq!(back.body, e.body, "{}", s.note);
                wire_type(&e.body)
            }),
            "client" => decode::<ServerMessage>(s.line.as_bytes()).map(|e| {
                let back: Envelope<ServerMessage> =
                    decode(encode(e.v, e.id, &e.body).trim_end().as_bytes()).unwrap();
                assert_eq!(back.body, e.body, "{}", s.note);
                wire_type(&e.body)
            }),
            other => panic!("bad direction {other}"),
        };
        match got {
            Ok(ty) => {
                assert!(s.valid, "{}: decoded but should be rejected", s.note);
                assert_eq!(ty, s.ty, "{}", s.note);
            }
            Err(_) => assert!(!s.valid, "{}: rejected but should decode", s.note),
        }
    }
}
