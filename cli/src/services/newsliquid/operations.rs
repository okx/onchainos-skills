//! NewsLiquid operation parsing and dispatch for the service CLI.
use super::{flow, PurchaseKind};
use crate::commands::sink::CodedError;
use anyhow::Result;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};

pub(crate) fn require_service(service: &str) -> Result<()> {
    if [
        super::ID,
        "News Liquid",
        "6551",
        "6551Team",
        "牛子液体",
        "新闻液体",
    ]
    .iter()
    .any(|name| name.eq_ignore_ascii_case(service.trim()))
    {
        return Ok(());
    }
    Err(CodedError::new(
        "service_not_registered",
        Some("service"),
        "Choose a registered third-party service.",
    )
    .into())
}

pub(crate) fn check_params(params: &Value) -> Result<()> {
    if !params.is_object() {
        return Err(CodedError::invalid_input("params", "Provide a JSON parameter object.").into());
    }
    Ok(())
}

fn check_operation(operation: &str, confirmed: bool) -> Result<()> {
    let metadata = describe();
    let entry = metadata["operations"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["operation"] == operation))
        .ok_or_else(|| {
            CodedError::invalid_input(
                "operation",
                "Choose a supported operation for this service.",
            )
        })?;
    if entry["requiresConfirmation"] == true && !confirmed {
        return Err(CodedError::new(
            "service_operation_requires_confirmation",
            Some("confirmed"),
            "Obtain the user's confirmation before this service operation.",
        )
        .into());
    }
    Ok(())
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| {
        CodedError::invalid_input(
            "params",
            "Provide the documented parameters for this service operation.",
        )
        .into()
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LoginParams {
    #[serde(default)]
    confirm_wallet_warning: bool,
    wallet: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateParams {
    kind: PurchaseKind,
    product_id: u64,
    operation_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelParams {
    #[serde(default)]
    confirm_wallet_warning: bool,
    wallet: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareParams {
    kind: PurchaseKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayParams {
    approval: flow::PaymentApproval,
}

fn required_id(id: Option<String>) -> Result<String> {
    id.filter(|value| !value.is_empty()).ok_or_else(|| {
        CodedError::invalid_input("id", "Provide this operation's original service ID.").into()
    })
}

fn no_id(id: &Option<String>) -> Result<()> {
    if id.is_some() {
        return Err(CodedError::invalid_input("id", "This operation does not take an ID.").into());
    }
    Ok(())
}

pub(crate) async fn call(
    operation: &str,
    id: Option<String>,
    params: Value,
    confirmed: bool,
) -> Result<Value> {
    check_params(&params)?;
    check_operation(operation, confirmed)?;
    let command = match operation {
        "status" => {
            no_id(&id)?;
            parse::<Empty>(params)?;
            return super::status();
        }
        "token" => {
            no_id(&id)?;
            parse::<Empty>(params)?;
            return super::token();
        }
        "login" => {
            no_id(&id)?;
            let p: LoginParams = parse(params)?;
            flow::Command::Login {
                warning: p.confirm_wallet_warning,
                wallet: p.wallet,
            }
        }
        "create-order" => {
            no_id(&id)?;
            let p: CreateParams = parse(params)?;
            flow::Command::Create {
                kind: p.kind,
                product: p.product_id,
                key: p.operation_key,
            }
        }
        "prepare-payment" => {
            let p: PrepareParams = parse(params)?;
            flow::Command::Prepare {
                id: required_id(id)?,
                kind: p.kind,
            }
        }
        "pay" => {
            let p: PayParams = parse(params)?;
            flow::Command::Pay {
                id: required_id(id)?,
                approval: p.approval,
                confirmed,
            }
        }
        "cancel-renewal" => {
            let p: CancelParams = parse(params)?;
            flow::Command::Cancel {
                id: required_id(id)?,
                confirmed,
                warning: p.confirm_wallet_warning,
                wallet: p.wallet,
            }
        }
        _ => return flow::request(operation, id, params, confirmed).await,
    };
    flow::run(command).await
}

fn describe() -> Value {
    let catalog: Value =
        serde_json::from_str(include_str!("endpoints.json")).expect("service catalog");
    let excluded = [
        "auth-challenge",
        "auth-login",
        "billing-orders",
        "billing-orders-id-pay",
        "billing-subscriptions-id-cancel",
    ];
    let mut operations = vec![
        json!({"operation":"status","readOnly":true}),
        json!({"operation":"token","readOnly":true}),
    ];
    for operation in [
        "login",
        "create-order",
        "prepare-payment",
        "pay",
        "cancel-renewal",
    ] {
        operations.push(json!({"operation":operation,"readOnly":false,
            "requiresConfirmation":matches!(operation,"pay"|"cancel-renewal")}));
    }
    for (name, item) in catalog.as_object().unwrap() {
        if !excluded.contains(&name.as_str()) {
            let read_only = !matches!(
                name.as_str(),
                "twitter-watch-add" | "twitter-watch-delete" | "billing-orders-id-reconcile"
            );
            operations.push(json!({"operation":name,"readOnly":read_only,"method":item["method"],
                "requiresConfirmation":matches!(name.as_str(),"twitter-watch-add"|"twitter-watch-delete")}));
        }
    }
    json!({"id":super::ID,"name":"NewsLiquid / 6551","transports":["https"],
        "operations":operations})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_aliases_and_metadata_are_newsliquid_only() {
        for name in [
            "NewsLiquid",
            "News Liquid",
            "6551",
            "6551team",
            "牛子液体",
            "新闻液体",
        ] {
            require_service(name).unwrap();
        }
        let error = require_service("https://other.invalid").unwrap_err();
        assert_eq!(
            error.downcast_ref::<CodedError>().unwrap().code,
            "service_not_registered"
        );
        assert_eq!(describe()["id"], super::super::ID);
        assert_eq!(describe()["operations"].as_array().unwrap().len(), 41);
    }

    #[tokio::test]
    async fn token_rejects_wallet_overrides_and_ids_before_credential_access() {
        for (id, params) in [
            (Some("other-wallet".into()), json!({})),
            (None, json!({"wallet":"other-wallet"})),
            (None, json!({"token":"not-accepted"})),
        ] {
            let error = call("token", id, params, false).await.unwrap_err();
            assert_eq!(
                error.downcast_ref::<CodedError>().unwrap().code,
                "invalid_input"
            );
        }
    }

    #[tokio::test]
    async fn unsupported_operations_and_unconfirmed_writes_stop_before_wallet_access() {
        for operation in [
            "pay",
            "cancel-renewal",
            "twitter-watch-add",
            "twitter-watch-delete",
        ] {
            let error = call(
                operation,
                Some("id".into()),
                json!({"confirmed":true}),
                false,
            )
            .await
            .unwrap_err();
            assert_eq!(
                error.downcast_ref::<CodedError>().unwrap().code,
                "service_operation_requires_confirmation"
            );
        }
        let error = call("unsupported", None, json!({}), false)
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<CodedError>().unwrap().field.as_deref(),
            Some("operation")
        );
        let error = call("status", None, json!([]), false).await.unwrap_err();
        assert_eq!(
            error.downcast_ref::<CodedError>().unwrap().field.as_deref(),
            Some("params")
        );
    }

    #[test]
    fn provider_parameter_shapes_preserve_existing_choices_and_reject_secret_imports() {
        let create: CreateParams =
            parse(json!({"kind":"one_time","productId":3,"operationKey":"original"})).unwrap();
        assert!(matches!(create.kind, PurchaseKind::OneTime));
        assert_eq!(create.product_id, 3);
        assert_eq!(create.operation_key, "original");
        let login: LoginParams = parse(json!({"confirmWalletWarning":true})).unwrap();
        assert!(login.confirm_wallet_warning);
        let cancel: CancelParams =
            parse(json!({"wallet":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","confirmWalletWarning":true})).unwrap();
        assert!(cancel.wallet.is_some() && cancel.confirm_wallet_warning);
        assert!(parse::<LoginParams>(json!({"token":"not-accepted"})).is_err());
        assert!(parse::<CreateParams>(
            json!({"kind":"one_time","productId":"3","operationKey":"original"})
        )
        .is_err());
    }

    #[tokio::test]
    async fn operation_dispatch_keeps_confirmation_guards() {
        let pay = flow::run(flow::Command::Pay { id:"original".into(), approval: serde_json::from_value(json!({"wallet":"w","orderId":"original","kind":"one_time","quote":{},"accepted":{},"allowance":null,"expiresAt":0,"autoRenewTerms":null,"requiresConfirmation":true})).unwrap(), confirmed:false })
            .await
            .unwrap_err();
        assert!(pay
            .to_string()
            .contains("explicit_payment_confirmation_required"));
        let cancel = call("cancel-renewal", Some("original".into()), json!({}), false)
            .await
            .unwrap_err();
        assert_eq!(
            cancel.downcast_ref::<CodedError>().unwrap().code,
            "service_operation_requires_confirmation"
        );
    }
}
