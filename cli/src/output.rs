use serde::Serialize;

#[derive(Serialize)]
struct JsonOutput<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Print a success response: `{ "ok": true }`
pub fn success_empty() {
    let out: JsonOutput<()> = JsonOutput {
        ok: true,
        data: None,
        error: None,
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

/// Print a success response with data: `{ "ok": true, "data": ... }`
pub fn success<T: Serialize>(data: T) {
    let out = JsonOutput {
        ok: true,
        data: Some(data),
        error: None,
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

/// Print an error response: `{ "ok": false, "error": "<msg>" }`
pub fn error(msg: &str) {
    let out: JsonOutput<()> = JsonOutput {
        ok: false,
        data: None,
        error: Some(msg.to_string()),
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
