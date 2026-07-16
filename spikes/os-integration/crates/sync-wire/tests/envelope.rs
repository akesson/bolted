//! Envelope round-trip + unknown-version refusal (deliverable 3).

use sync_wire::{
    ClientFrame, ErrorWire, FieldName, RawWire, Request, Response, SCHEMA_VERSION, ServerEnvelope,
    ServerFrame, WireError, decode, encode, probe_version,
};

#[test]
fn client_frame_roundtrips() {
    let frame = ClientFrame {
        v: SCHEMA_VERSION,
        seq: 7,
        req: Request::TrySet {
            draft: 3,
            field: FieldName::Label,
            value: RawWire::Text("Photos".to_string()),
        },
    };
    let line = encode(&frame).expect("encodes");
    assert!(!line.contains('\n'), "one frame is one line");
    let back: ClientFrame = decode(&line).expect("decodes");
    assert_eq!(back, frame);
}

#[test]
fn bool_and_text_raws_are_self_describing() {
    let flag = encode(&RawWire::Flag(true)).expect("encodes");
    let text = encode(&RawWire::Text("true".to_string())).expect("encodes");
    assert_eq!(flag, "true");
    assert_eq!(text, "\"true\"");
    let back_flag: RawWire = serde_json::from_str(&flag).expect("decodes");
    let back_text: RawWire = serde_json::from_str(&text).expect("decodes");
    assert_eq!(back_flag, RawWire::Flag(true));
    assert_eq!(back_text, RawWire::Text("true".to_string()));
}

#[test]
fn server_frame_roundtrips_with_structured_params() {
    let env = ServerEnvelope {
        v: SCHEMA_VERSION,
        frame: ServerFrame::Response {
            re: 7,
            resp: Box::new(Response::SetOutcome {
                error: Some(ErrorWire {
                    key: "too_long".to_string(),
                    params: vec![
                        ("max".to_string(), "30".to_string()),
                        ("actual".to_string(), "31".to_string()),
                    ],
                }),
            }),
        },
    };
    let line = encode(&env).expect("encodes");
    let back: ServerEnvelope = decode(&line).expect("decodes");
    assert_eq!(back, env);
}

#[test]
fn unknown_version_is_a_typed_refusal_before_any_body_parse() {
    // The body here is deliberately garbage a full parse would choke on: the version gate must
    // fire first (parse-don't-validate).
    let line = r#"{"v":999,"seq":1,"req":{"t":"no_such_verb","junk":[]}}"#;
    assert_eq!(
        probe_version(line),
        Err(WireError::UnknownVersion { got: 999 })
    );
    let decoded: Result<ClientFrame, WireError> = decode(line);
    assert_eq!(decoded, Err(WireError::UnknownVersion { got: 999 }));
}

#[test]
fn a_frame_without_a_version_is_malformed() {
    assert!(matches!(
        probe_version(r#"{"seq":1}"#),
        Err(WireError::Json(_))
    ));
}
