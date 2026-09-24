//! Device pairing ("Easy connect"): a TV that cannot use a keyboard asks the
//! server for a short code, shows it (and a QR payload) on screen, and a
//! signed-in phone approves the code. The TV then polls and claims a real
//! session for the approving user — the same session a password login creates.

use crate::{
    AppState,
    auth::{self, Auth, Policy, User},
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use rand_core::{OsRng, RngCore};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{Value, json};

/// Codes are read off a television and typed on a phone, so the alphabet drops
/// the characters that are easy to confuse (I/O/0/1). 8 characters is ~40 bits.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LENGTH: usize = 8;
const TTL_SECONDS: i64 = 300;
const SESSION_SECONDS: i64 = 30 * 86400;

fn new_code() -> String {
    let mut bytes = [0u8; CODE_LENGTH];
    OsRng.fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char)
        .collect()
}

/// Codes are entered by hand, so accept any spacing/case the user produces.
fn normalize(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Initiation {
    pub device_name: Option<String>,
}

/// Called by the device that wants to sign in (an Apple TV). No credentials:
/// the reply is only useful once a signed-in user approves the code.
pub async fn initiate(
    State(state): State<AppState>,
    Json(input): Json<Initiation>,
) -> Result<Json<Value>> {
    let name: String = input
        .device_name
        .unwrap_or_default()
        .trim()
        .chars()
        .take(64)
        .collect();
    let code = new_code();
    let issued = code.clone();
    let now = auth::now();
    state
        .db
        .call(move |c| {
            c.execute("DELETE FROM pairings WHERE expires_at<=?1", [now])?;
            c.execute(
                "INSERT INTO pairings(code,device_name,created_at,expires_at) VALUES (?1,?2,?3,?4)",
                rusqlite::params![code, name, now, now + TTL_SECONDS],
            )?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({"Code":issued,"ExpiresIn":TTL_SECONDS})))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CodeQuery {
    pub code: String,
}

/// Polled by the waiting device. Answers whether a user has approved yet, and
/// hands over the session exactly once — claiming the code consumes it.
pub async fn connect(
    State(state): State<AppState>,
    Query(query): Query<CodeQuery>,
) -> Result<Json<Value>> {
    let code = normalize(&query.code);
    if code.len() != CODE_LENGTH {
        return Err(Error::missing());
    }
    let now = auth::now();
    let lookup = code.clone();
    let pending = state
        .db
        .call(move |c| {
            c.execute("DELETE FROM pairings WHERE expires_at<=?1", [now])?;
            Ok(c.query_row(
                "SELECT p.user_id,u.name,u.is_admin FROM pairings p LEFT JOIN users u ON u.id=p.user_id WHERE p.code=?1",
                [&lookup],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<bool>>(2)?,
                    ))
                },
            )
            .optional()?)
        })
        .await?
        .ok_or_else(Error::missing)?;
    let Some(user_id) = pending.0 else {
        return Ok(Json(json!({"Authenticated":false})));
    };
    let user = User {
        id: user_id,
        name: pending.1.ok_or_else(Error::missing)?,
        policy: Policy {
            is_administrator: pending.2.unwrap_or(false),
        },
    };
    let approver = user.id.clone();
    let token = format!("{}{}", auth::id(), auth::id());
    let hash = auth::digest(&token);
    let issued = token.clone();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM sessions WHERE expires_at<=?1", [now])?;
            tx.execute(
                "INSERT INTO sessions VALUES (?1,?2,?3)",
                rusqlite::params![hash, approver, now + SESSION_SECONDS],
            )?;
            tx.execute("DELETE FROM pairings WHERE code=?1", [&code])?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({
        "Authenticated":true,"User":user,"AccessToken":issued,"ServerId":state.server_id
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Approval {
    pub code: String,
}

/// Called by a signed-in client (phone, tablet, Mac) to grant the waiting
/// device the approver's own session.
pub async fn approve(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<Approval>,
) -> Result<Json<Value>> {
    let code = normalize(&input.code);
    if code.len() != CODE_LENGTH {
        return Err(Error::missing());
    }
    let approver = auth.user.id.clone();
    let now = auth::now();
    state
        .db
        .call(move |c| {
            c.execute("DELETE FROM pairings WHERE expires_at<=?1", [now])?;
            let existing = c
                .query_row("SELECT user_id FROM pairings WHERE code=?1", [&code], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .optional()?
                .ok_or_else(Error::missing)?;
            // Whoever approves first owns the session; a later approver must
            // not be able to redirect a code the user already confirmed.
            if existing.is_some_and(|owner| owner != approver) {
                return Err(Error(
                    StatusCode::CONFLICT,
                    "This code was already approved".into(),
                ));
            }
            c.execute(
                "UPDATE pairings SET user_id=?1,approved_at=?2 WHERE code=?3",
                rusqlite::params![approver, now, code],
            )?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({"Approved":true})))
}
