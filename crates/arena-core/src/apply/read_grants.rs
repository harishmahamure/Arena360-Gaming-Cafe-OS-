//! `audit.read_granted`: the Hub records each read elevation a manager's PIN granted
//! (system actor only), so the grant shows in the audit log like any other change.

use arena_proto::{AppError, AuditReadGranted, ErrorCode, Intent};
use arena_store::{ChangeOp, NewReadGrant, RowKey, StoreTx, current_branch_id, insert_read_grant};

use super::Target;
use super::mutate::Written;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Target, AppError> {
    Ok(Target::Branch(current_branch_id(tx)?))
}

pub(super) fn validate(
    intent: &Intent,
    g: &AuditReadGranted,
    branch: Option<&String>,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "audit.read_granted takes no base_rev"));
    }
    if !g.permission.is_read_elevation() {
        return Err(invalid(
            "permission",
            "only a read permission is granted for reading",
        ));
    }
    if g.requester_staff_id.trim().is_empty() {
        return Err(invalid(
            "requester_staff_id",
            "name the requesting staff member",
        ));
    }
    if g.approver_staff_id.trim().is_empty() || g.approver_staff_id == g.requester_staff_id {
        return Err(invalid(
            "approver_staff_id",
            "name another staff member as the approver",
        ));
    }
    if branch.is_none() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "the branch has not been bootstrapped",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    g: &AuditReadGranted,
    branch: Option<&String>,
) -> Result<Vec<Written>, AppError> {
    let branch_id = branch.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "read grant validated without a branch")
    })?;
    let grant_id = ulid::Ulid::generate().to_string();
    insert_read_grant(
        tx,
        &NewReadGrant {
            grant_id: &grant_id,
            branch_id,
            permission: g.permission.as_str(),
            requester_staff_id: &g.requester_staff_id,
            approver_staff_id: &g.approver_staff_id,
            verifier_version: g.verifier_version,
        },
    )?;
    Ok(vec![Written {
        key: RowKey::ReadGrants { grant_id },
        op: ChangeOp::Insert,
        before: None,
    }])
}
