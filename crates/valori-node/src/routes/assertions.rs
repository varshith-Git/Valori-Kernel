use super::meta::MetaOps;
use axum::{extract::Path, response::Response, Json};
use valori_rag::community::{verify_structured_claims, VerificationReceipt, VerifyClaimRequest};

pub async fn verify<O: MetaOps>(
    ops: &O,
    req: VerifyClaimRequest,
) -> Result<Json<VerificationReceipt>, Response> {
    let r = verify_structured_claims(
        (
            &req.left.subject,
            &req.left.predicate,
            &req.left.object,
            req.left.negated,
            req.left.time_scope.as_deref(),
        ),
        (
            &req.right.subject,
            &req.right.predicate,
            &req.right.object,
            req.right.negated,
            req.right.time_scope.as_deref(),
        ),
        &req.left_assertion_id,
        &req.right_assertion_id,
        req.evidence_refs,
    );
    let key = format!("assertion-verification:{}", r.verification_id);
    ops.set_meta(
        key,
        serde_json::to_value(&r).unwrap_or(serde_json::Value::Null),
    )
    .await?;
    Ok(Json(r))
}

pub async fn get<O: MetaOps>(ops: &O, Path(id): Path<String>) -> Json<Option<VerificationReceipt>> {
    let key = format!("assertion-verification:{id}");
    Json(
        ops.get_meta(&key)
            .await
            .and_then(|v| serde_json::from_value(v).ok()),
    )
}
