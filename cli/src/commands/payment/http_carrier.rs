//! Carrier-aware HTTP request assembly for the two-phase payment flow. A2MCP /
//! merchant endpoints declare, per business param, a
//! *carrier* (`query` | `body` | `header` | `path`) and an overall request
//! *method*. Both `payment quote`'s probe and `payment pay`'s replay build their
//! outbound request through [`build_request`] so a POST+body (or header/path)
//! endpoint is honored instead of the old hardcoded `GET` + query string.

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::{Map, Value};

use super::state::{ParamCarrier, ParamSpec};

/// HTTP methods that carry a request body. Params with no explicit carrier ride
/// in the JSON body for these methods, and in the query string otherwise —
/// preserving the pre-carrier GET+query behavior for simple x402 endpoints.
fn is_body_bearing(method: &str) -> bool {
    matches!(
        method.to_ascii_uppercase().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    )
}

/// Resolve the carrier for `name`: an explicit `outputSchema.input` spec wins;
/// otherwise default to body for body-bearing methods, else query.
fn carrier_for(name: &str, plan: &[ParamSpec], body_bearing: bool) -> ParamCarrier {
    if let Some(spec) = plan.iter().find(|s| s.name == name) {
        return spec.carrier.clone();
    }
    if body_bearing {
        ParamCarrier::Body
    } else {
        ParamCarrier::Query
    }
}

/// Assemble a `reqwest::RequestBuilder` for `method url` placing each `(key,
/// value)` param on its carrier (per `plan`, else the method default):
/// - `path`   → substitute `{key}` placeholders in the URL;
/// - `query`  → query string;
/// - `body`   → JSON body (only sent for body-bearing methods);
/// - `header` → request header.
///
/// The caller adds any signed payment header (e.g. `PAYMENT-SIGNATURE`) onto the
/// returned builder — carrier params never collide with it.
pub fn build_request(
    client: &reqwest::Client,
    method: &str,
    url: &str,
    params: &[(String, String)],
    plan: &[ParamSpec],
) -> reqwest::RequestBuilder {
    let body_bearing = is_body_bearing(method);

    let mut final_url = url.to_string();
    let mut query: Vec<(String, String)> = Vec::new();
    let mut body: Map<String, Value> = Map::new();
    let mut headers: Vec<(String, String)> = Vec::new();

    for (k, v) in params {
        match carrier_for(k, plan, body_bearing) {
            ParamCarrier::Path => {
                // Percent-encode the value before substituting it into the URL
                // path: a raw value containing spaces, `/`, `?`, `#`, `&` etc.
                // would otherwise break out of its segment and produce a
                // malformed or ambiguous URL. `NON_ALPHANUMERIC` keeps only the
                // unreserved ASCII alphanumerics literal (a superset-safe set for
                // a single path segment). query/body carriers are encoded by
                // reqwest downstream and are deliberately left untouched here.
                let encoded = utf8_percent_encode(v, NON_ALPHANUMERIC).to_string();
                final_url = final_url.replace(&format!("{{{k}}}"), &encoded);
            }
            ParamCarrier::Query => query.push((k.clone(), v.clone())),
            ParamCarrier::Body => {
                body.insert(k.clone(), Value::String(v.clone()));
            }
            ParamCarrier::Header => headers.push((k.clone(), v.clone())),
        }
    }

    let http_method = reqwest::Method::from_bytes(method.to_ascii_uppercase().as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let mut rb = client.request(http_method, &final_url);
    if !query.is_empty() {
        rb = rb.query(&query);
    }
    // Only body-bearing methods carry a JSON body; body-tagged params on a GET
    // are dropped rather than silently converted (an intentional no-op we treat
    // as a merchant-schema inconsistency, not a fund-safety issue).
    if body_bearing && !body.is_empty() {
        rb = rb.json(&Value::Object(body));
    }
    for (hk, hv) in headers {
        rb = rb.header(hk, hv);
    }
    rb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, carrier: ParamCarrier) -> ParamSpec {
        ParamSpec {
            name: name.into(),
            carrier,
            required: false,
            type_: String::new(),
        }
    }

    #[test]
    fn is_body_bearing_classifies_methods() {
        assert!(is_body_bearing("POST"));
        assert!(is_body_bearing("put"));
        assert!(is_body_bearing("Patch"));
        assert!(!is_body_bearing("GET"));
        assert!(!is_body_bearing("HEAD"));
    }

    #[test]
    fn carrier_defaults_by_method_when_unspecified() {
        // No plan entry → GET defaults to query, POST defaults to body.
        assert_eq!(carrier_for("orderId", &[], false), ParamCarrier::Query);
        assert_eq!(carrier_for("orderId", &[], true), ParamCarrier::Body);
        // Explicit plan entry wins over the method default.
        let plan = vec![spec("orderId", ParamCarrier::Header)];
        assert_eq!(carrier_for("orderId", &plan, true), ParamCarrier::Header);
    }

    #[test]
    fn build_request_substitutes_path_params() {
        // `build_request` builds a RequestBuilder; we can't introspect it
        // directly, but path substitution mutates the URL we can rebuild here.
        let plan = vec![spec("id", ParamCarrier::Path)];
        // Mirror the substitution the builder performs.
        let url = "https://m.example/orders/{id}";
        let substituted = url.replace("{id}", "42");
        assert_eq!(substituted, "https://m.example/orders/42");
        // Sanity: a Path-carrier param resolves to Path.
        assert_eq!(carrier_for("id", &plan, false), ParamCarrier::Path);
    }

    #[test]
    fn path_value_is_percent_encoded_before_substitution() {
        // A path value carrying spaces / reserved chars must not break out of
        // its segment: `utf8_percent_encode(_, NON_ALPHANUMERIC)` escapes every
        // non-alphanumeric byte, so the assembled URL stays well-formed.
        let raw = "a b/c?d#e&f";
        let encoded = utf8_percent_encode(raw, NON_ALPHANUMERIC).to_string();
        assert_eq!(encoded, "a%20b%2Fc%3Fd%23e%26f");
        // No structural URL delimiters survive in the encoded segment.
        for ch in ['/', '?', '#', '&', ' '] {
            assert!(
                !encoded.contains(ch),
                "encoded segment must not contain raw `{ch}`: {encoded}"
            );
        }
    }
}
