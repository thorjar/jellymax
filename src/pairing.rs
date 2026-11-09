//! EasyLink device sign-in, wire-compatible with Jellyfin Quick Connect.

use crate::{
    AppState,
    auth::{self, Auth, Policy, User},
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use rand_core::{OsRng, RngCore};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{Value, json};

const CODE_LENGTH: usize = 6;
const TTL_SECONDS: i64 = 10 * 60;
const SESSION_SECONDS: i64 = 30 * 86400;

fn new_code() -> String {
    // Match Jellyfin's six-digit, numeric user-facing code.
    format!("{:06}", 100_000 + (OsRng.next_u32() % 900_000))
}

fn new_secret() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode_upper(bytes)
}

fn normalize_code(code: &str) -> String {
    code.chars().filter(char::is_ascii_digit).collect()
}

fn client_field(headers: &HeaderMap, name: &str) -> Option<String> {
    let value = headers
        .get("X-Emby-Authorization")
        .or_else(|| headers.get("Authorization"))?
        .to_str()
        .ok()?;
    let fields = value
        .strip_prefix("MediaBrowser ")
        .or_else(|| value.strip_prefix("Emby "))
        .unwrap_or(value);
    fields.split(',').find_map(|field| {
        let (key, value) = field.trim().split_once('=')?;
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().trim_matches('"').chars().take(128).collect())
    })
}

#[allow(clippy::too_many_arguments)]
fn result(
    authenticated: bool,
    secret: String,
    code: String,
    created_at: i64,
    device_id: String,
    device_name: String,
    app_name: String,
    app_version: String,
) -> Value {
    let date_added = chrono::DateTime::from_timestamp(created_at, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    json!({
        "Authenticated": authenticated,
        "Secret": secret,
        "Code": code,
        "DeviceId": device_id,
        "DeviceName": device_name,
        "AppName": app_name,
        "AppVersion": app_version,
        "DateAdded": date_added
    })
}

/// Jellyfin clients use this to decide whether to offer Quick Connect.
pub async fn enabled() -> Json<bool> {
    Json(true)
}

/// Starts an EasyLink request. Jellyfin sends client/device metadata in its
/// MediaBrowser authorization header and no JSON body.
pub async fn initiate(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let device_id = client_field(&headers, "DeviceId").unwrap_or_else(auth::id);
    let device_name = client_field(&headers, "Device").unwrap_or_else(|| "Unknown device".into());
    let app_name = client_field(&headers, "Client").unwrap_or_else(|| "Unknown client".into());
    let app_version = client_field(&headers, "Version").unwrap_or_default();
    let secret = new_secret();
    let candidates: Vec<String> = (0..12).map(|_| new_code()).collect();
    let created_at = auth::now();
    let expires_at = created_at + TTL_SECONDS;
    let stored_secret = secret.clone();
    let stored_device_id = device_id.clone();
    let stored_device_name = device_name.clone();
    let stored_app_name = app_name.clone();
    let stored_app_version = app_version.clone();
    let code = state.db.call(move |connection| {
        connection.execute("DELETE FROM pairings WHERE expires_at<=?1", [created_at])?;
        for code in candidates {
            let inserted = connection.execute(
                "INSERT OR IGNORE INTO pairings(code,secret,device_id,device_name,app_name,app_version,created_at,expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params![code,stored_secret,stored_device_id,stored_device_name,stored_app_name,stored_app_version,created_at,expires_at],
            )?;
            if inserted == 1 {
                return Ok(code);
            }
        }
        Err(Error::internal("Could not allocate an EasyLink code"))
    }).await?;
    Ok(Json(result(
        false,
        secret,
        code,
        created_at,
        device_id,
        device_name,
        app_name,
        app_version,
    )))
}

#[derive(Deserialize)]
pub struct SecretQuery {
    #[serde(alias = "Secret")]
    pub secret: String,
}

/// Returns request state only. Access tokens are issued by the separate
/// AuthenticateWithQuickConnect exchange after approval.
pub async fn connect(
    State(state): State<AppState>,
    Query(query): Query<SecretQuery>,
) -> Result<Json<Value>> {
    if query.secret.len() > 128 {
        return Err(Error::missing());
    }
    let now = auth::now();
    let secret = query.secret;
    let row = state.db.call(move |connection| {
        connection.execute("DELETE FROM pairings WHERE expires_at<=?1", [now])?;
        Ok(connection.query_row(
            "SELECT user_id IS NOT NULL,secret,code,created_at,device_id,device_name,app_name,app_version FROM pairings WHERE secret=?1",
            [&secret],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?)),
        ).optional()?)
    }).await?.ok_or_else(Error::missing)?;
    Ok(Json(result(
        row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7,
    )))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizeQuery {
    #[serde(alias = "Code")]
    pub code: String,
    #[serde(default, alias = "UserId")]
    pub user_id: Option<String>,
}

async fn authorize_code(
    auth: Auth,
    state: AppState,
    code: String,
    requested_user: Option<String>,
) -> Result<Json<bool>> {
    let code = normalize_code(&code);
    if code.len() != CODE_LENGTH {
        return Err(Error::missing());
    }
    let user_id = requested_user.unwrap_or_else(|| auth.user.id.clone());
    if user_id != auth.user.id {
        auth.admin()?;
    }
    let now = auth::now();
    state
        .db
        .call(move |connection| {
            let tx = connection.transaction()?;
            tx.execute("DELETE FROM pairings WHERE expires_at<=?1", [now])?;
            let user_exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE id=?1)",
                [&user_id],
                |row| row.get(0),
            )?;
            if !user_exists {
                return Err(Error::forbidden());
            }
            let owner = tx
                .query_row(
                    "SELECT user_id FROM pairings WHERE code=?1",
                    [&code],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .ok_or_else(Error::missing)?;
            if owner.is_some_and(|owner| owner != user_id) {
                return Err(Error(
                    StatusCode::CONFLICT,
                    "This EasyLink code was already authorized".into(),
                ));
            }
            tx.execute(
                "UPDATE pairings SET user_id=?1,approved_at=?2 WHERE code=?3",
                rusqlite::params![user_id, now, code],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(Json(true))
}

/// Jellyfin-compatible approval endpoint used by Jellyfin clients and SDKs.
pub async fn authorize(
    auth: Auth,
    State(state): State<AppState>,
    Query(query): Query<AuthorizeQuery>,
) -> Result<Json<bool>> {
    authorize_code(auth, state, query.code, query.user_id).await
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LegacyApproval {
    pub code: String,
}

/// Compatibility alias for Jellymax clients released with the initial pairing prototype.
pub async fn approve(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<LegacyApproval>,
) -> Result<Json<bool>> {
    authorize_code(auth, state, input.code, None).await
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Exchange {
    pub secret: String,
}

/// Exchanges an approved secret for a normal Jellymax session. The secret is
/// single-use, so replaying this request cannot create more sessions.
pub async fn authenticate(
    State(state): State<AppState>,
    Json(input): Json<Exchange>,
) -> Result<Json<Value>> {
    if input.secret.len() > 128 {
        return Err(Error::missing());
    }
    let now = auth::now();
    let secret = input.secret;
    let token = format!("{}{}", auth::id(), auth::id());
    let token_hash = auth::digest(&token);
    let user = state.db.call(move |connection| {
        let tx = connection.transaction()?;
        tx.execute("DELETE FROM pairings WHERE expires_at<=?1", [now])?;
        let user = tx.query_row(
            "SELECT u.id,u.name,u.is_admin FROM pairings p JOIN users u ON u.id=p.user_id WHERE p.secret=?1 AND p.approved_at IS NOT NULL",
            [&secret],
            |row| Ok(User { id:row.get(0)?, name:row.get(1)?, policy:Policy { is_administrator:row.get(2)? } }),
        ).optional()?.ok_or_else(Error::missing)?;
        tx.execute("DELETE FROM sessions WHERE expires_at<=?1", [now])?;
        tx.execute(
            "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES (?1,?2,?3)",
            rusqlite::params![token_hash,user.id,now + SESSION_SECONDS],
        )?;
        tx.execute("DELETE FROM pairings WHERE secret=?1", [&secret])?;
        tx.commit()?;
        Ok(user)
    }).await?;
    Ok(Json(
        json!({"User":user,"AccessToken":token,"ServerId":state.server_id}),
    ))
}
