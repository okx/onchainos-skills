use super::*;

pub(super) fn parse_probe_input(
    routing_json: &str,
    params_json: &str,
) -> Result<ProbeInput, ContractError> {
    let routing: RoutingPayload =
        serde_json::from_str(routing_json).map_err(|error| ContractError {
            code: "invalid_a2mcp_routing",
            message: format!("routing JSON is invalid: {error}"),
        })?;
    if routing.schema_version != 1 {
        return Err(ContractError {
            code: "invalid_a2mcp_routing",
            message: "schemaVersion must be the integer 1".to_string(),
        });
    }
    let object = routing
        .service_snapshot
        .as_object()
        .ok_or_else(|| ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot must be an object".to_string(),
        })?;
    if !object
        .get("serviceType")
        .and_then(Value::as_str)
        .is_some_and(|service_type| service_type.eq_ignore_ascii_case("A2MCP"))
    {
        return Err(ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot.serviceType must equal A2MCP".to_string(),
        });
    }
    let endpoint = object
        .get("endpoint")
        .and_then(Value::as_str)
        .ok_or_else(|| ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot.endpoint must be a URL string".to_string(),
        })?;
    let endpoint = Url::parse(endpoint).map_err(|error| ContractError {
        code: "invalid_a2mcp_routing",
        message: format!("serviceSnapshot.endpoint is invalid: {error}"),
    })?;
    if endpoint.scheme() != "https" {
        return Err(ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot.endpoint must use HTTPS".to_string(),
        });
    }
    let typed_params = serde_json::from_str::<Value>(params_json)
        .map_err(|error| ContractError {
            code: "invalid_a2mcp_params",
            message: format!("params JSON is invalid: {error}"),
        })?
        .as_object()
        .cloned()
        .ok_or_else(|| ContractError {
            code: "invalid_a2mcp_params",
            message: "params JSON must be an object".to_string(),
        })?;
    let request_spec = routing.request_spec.clone();
    let param_plan = request_spec
        .as_ref()
        .map(|spec| spec.fields.clone())
        .unwrap_or_else(|| {
            object
                .get("outputSchema")
                .and_then(|value| value.get("input"))
                .map(parse_fields)
                .unwrap_or_default()
        });
    let required_any_of = request_spec
        .as_ref()
        .map(|spec| spec.required_any_of.clone())
        .unwrap_or_else(|| {
            string_array(
                object
                    .get("outputSchema")
                    .and_then(|schema| schema.get("requiredAnyOf")),
            )
        });
    let service_id = scalar_string(object.get("serviceId"))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot.serviceId is required".to_string(),
        })?;
    validate_typed_params(&typed_params, &param_plan)?;
    let raw = routing.service_snapshot.clone();
    let fallback_method = request_spec
        .and_then(|spec| spec.method)
        .or_else(|| {
            object
                .get("method")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            object
                .get("outputSchema")
                .and_then(|schema| schema.get("method"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        });
    let service_description = object.get("serviceDescription").and_then(Value::as_str);
    let method_was_defaulted =
        request_method_is_defaulted(service_description, fallback_method.as_deref());
    let method =
        resolve_request_method(service_description, &endpoint, fallback_method.as_deref())?;
    Ok(ProbeInput {
        snapshot: ServiceSnapshot {
            raw,
            service_id,
            service_name: object
                .get("serviceName")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned),
            endpoint,
            method,
            method_was_defaulted,
            asp_amount: scalar_string(object.get("feeAmount")),
            asp_symbol: object
                .get("feeTokenSymbol")
                .and_then(Value::as_str)
                .map(str::to_ascii_uppercase),
            param_plan,
            required_any_of,
        },
        typed_params,
    })
}

pub(super) fn to_payment_param_plan(
    plan: &[FieldConstraint],
    method: &str,
) -> Result<Vec<crate::commands::payment::state::ParamSpec>> {
    use crate::commands::payment::state::{ParamCarrier, ParamSpec};
    plan.iter()
        .map(|field| {
            let default_carrier = if method.eq_ignore_ascii_case("POST") {
                "body"
            } else {
                "query"
            };
            let carrier = match field.carrier.as_deref().unwrap_or(default_carrier) {
                "query" => ParamCarrier::Query,
                "body" => ParamCarrier::Body,
                "header" => ParamCarrier::Header,
                "path" => ParamCarrier::Path,
                other => {
                    return Err(anyhow!(
                        "invalid_a2mcp_params: unsupported carrier `{other}`"
                    ))
                }
            };
            Ok(ParamSpec {
                name: field.name.clone(),
                carrier,
                required: field.required,
                type_: field.type_.clone(),
            })
        })
        .collect()
}

pub(super) fn outstanding_input(
    mut required: InputRequired,
    params: &Map<String, Value>,
) -> Option<InputRequired> {
    let alternatives = &required.required_any_of;
    required.fields.retain(|field| {
        field.required && !alternatives.contains(&field.name) && !params.contains_key(&field.name)
    });
    if required
        .required_any_of
        .iter()
        .any(|name| params.contains_key(name))
    {
        required.required_any_of.clear();
    }
    if required.fields.is_empty() && required.required_any_of.is_empty() {
        None
    } else {
        Some(required)
    }
}

pub(super) fn discover_input_required(value: &Value) -> Option<InputRequired> {
    if let Some(required) = value
        .get("input_required")
        .filter(|value| value.is_object())
    {
        let fields = required.get("fields").map(parse_fields).unwrap_or_default();
        let required_any_of = string_array(required.get("requiredAnyOf"));
        if !fields.is_empty() || !required_any_of.is_empty() {
            return Some(InputRequired {
                fields,
                required_any_of,
                message: required
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                method: required
                    .get("method")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        value
                            .get("outputSchema")
                            .and_then(|schema| schema.get("method"))
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    }),
            });
        }
    }
    if value.get("status").and_then(Value::as_str) == Some("input_required") {
        let fields = value
            .get("fields")
            .or_else(|| value.get("requiredArgs"))
            .map(parse_fields)
            .unwrap_or_default();
        let required_any_of = string_array(value.get("requiredAnyOf"));
        if !fields.is_empty() || !required_any_of.is_empty() {
            return Some(InputRequired {
                fields,
                required_any_of,
                message: value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                method: value
                    .get("method")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            });
        }
    }
    let output_schema = value.get("outputSchema");
    let output_fields = output_schema
        .and_then(|schema| schema.get("input"))
        .map(parse_fields)
        .unwrap_or_default()
        .into_iter()
        .filter(|field| field.required)
        .collect::<Vec<_>>();
    let output_required_any_of =
        string_array(output_schema.and_then(|schema| schema.get("requiredAnyOf")));
    if !output_fields.is_empty() || !output_required_any_of.is_empty() {
        return Some(InputRequired {
            fields: output_fields,
            required_any_of: output_required_any_of,
            message: None,
            method: value
                .get("outputSchema")
                .and_then(|schema| schema.get("method"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        });
    }
    let missing = string_array(value.get("missingParams"));
    let names = if missing.is_empty() {
        string_array(value.get("required"))
    } else {
        missing
    };
    if names.is_empty() {
        return None;
    }
    Some(InputRequired {
        fields: names
            .into_iter()
            .map(|name| FieldConstraint {
                name,
                type_: default_string_type(),
                required: true,
                carrier: None,
                description: None,
            })
            .collect(),
        required_any_of: string_array(value.get("requiredAnyOf")),
        message: value
            .get("message")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        method: value
            .get("outputSchema")
            .and_then(|schema| schema.get("method"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

pub(super) fn parse_fields(value: &Value) -> Vec<FieldConstraint> {
    match value {
        Value::Array(items) => items.iter().filter_map(parse_field).collect(),
        Value::Object(fields) => fields
            .iter()
            .map(|(name, schema)| field_from_schema(name, schema))
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn parse_field(value: &Value) -> Option<FieldConstraint> {
    value
        .as_str()
        .map(|name| field_from_schema(name, &Value::Null))
        .or_else(|| {
            value
                .get("name")
                .and_then(Value::as_str)
                .map(|name| field_from_schema(name, value))
        })
}

pub(super) fn field_from_schema(name: &str, schema: &Value) -> FieldConstraint {
    FieldConstraint {
        name: name.to_string(),
        type_: schema
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("string")
            .to_string(),
        required: schema
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        carrier: schema
            .get("carrier")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        description: schema
            .get("description")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    }
}

pub(super) fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn validate_typed_params(
    params: &Map<String, Value>,
    plan: &[FieldConstraint],
) -> Result<(), ContractError> {
    for field in plan {
        if !is_supported_param_type(&field.type_) {
            return Err(ContractError {
                code: "invalid_a2mcp_routing",
                message: format!(
                    "requestSpec field `{}` has unsupported type `{}`",
                    field.name, field.type_
                ),
            });
        }
        let Some(value) = params.get(&field.name) else {
            continue;
        };
        if !typed_value_matches(value, &field.type_) {
            return Err(ContractError {
                code: "invalid_a2mcp_param_value",
                message: format!("parameter `{}` must be {}", field.name, field.type_),
            });
        }
    }
    Ok(())
}

pub(super) fn is_supported_param_type(value: &str) -> bool {
    matches!(
        value,
        "string" | "number" | "integer" | "boolean" | "object" | "array"
    )
}

pub(super) fn typed_value_matches(value: &Value, expected: &str) -> bool {
    match expected {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        _ => false,
    }
}

pub(super) fn scalar_string(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}
