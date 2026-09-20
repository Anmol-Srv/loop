use acp_server::cli::client::unwrap_envelope;
use serde_json::json;

#[test]
fn a_success_envelope_yields_its_data() {
    let body = json!({ "success": true, "data": { "key": "acp" } });
    assert_eq!(unwrap_envelope(body).unwrap(), json!({ "key": "acp" }));
}

#[test]
fn an_error_envelope_yields_the_server_message() {
    let body = json!({
        "success": false,
        "error": { "code": "CONFLICT", "message": "a project with key 'acp' already exists" }
    });
    assert_eq!(
        unwrap_envelope(body).unwrap_err(),
        "a project with key 'acp' already exists"
    );
}

#[test]
fn a_malformed_body_reports_clearly_rather_than_panicking() {
    assert!(unwrap_envelope(json!({ "nonsense": 1 })).is_err());
}
