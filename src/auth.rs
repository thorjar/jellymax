use crate::{
    AppState,
    db::Database,
    error::{Error, Result},
};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
};
use rand_core::OsRng;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct User {
    pub id: String,
    pub name: String,
    pub policy: Policy,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Policy {
    pub is_administrator: bool,
}
#[derive(Clone)]
pub struct Auth {
    pub user: User,
    pub token_hash: String,
}
impl Auth {
    pub fn admin(&self) -> Result<()> {
        if self.user.policy.is_administrator {
            Ok(())
        } else {
            Err(Error::forbidden())
        }
    }
    pub fn own(&self, user: &str) -> Result<()> {
        if self.user.id == user {
            Ok(())
        } else {
            Err(Error::forbidden())
        }
    }
}
impl FromRequestParts<AppState> for Auth {
    type Rejection = Error;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        // Credentials stay out of URLs, access logs, and referrers.
        let token = parts
            .headers
            .get("X-Emby-Token")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned)
            .or_else(|| {
                parts
                    .headers
                    .get("Authorization")
                    .or_else(|| parts.headers.get("X-Emby-Authorization"))
                    .and_then(|h| h.to_str().ok())
                    .and_then(parse_authorization)
            })
            .ok_or_else(Error::unauthorized)?;
        if token.len() > 256 {
            return Err(Error::unauthorized());
        }
        let token_hash = digest(&token);
        state.db.call(move |c| {
            let user = c.query_row(
                "SELECT u.id,u.name,u.is_admin FROM users u JOIN sessions s ON s.user_id=u.id WHERE s.token_hash=?1 AND s.expires_at>?2",
                rusqlite::params![token_hash,now()],
                |r| Ok(User { id:r.get(0)?,name:r.get(1)?,policy:Policy {is_administrator:r.get(2)?} })
            ).optional()?.ok_or_else(Error::unauthorized)?;
            Ok(Self { user, token_hash })
        }).await
    }
}
fn parse_authorization(value: &str) -> Option<String> {
    if let Some((scheme, token)) = value.split_once(' ') {
        if scheme.eq_ignore_ascii_case("Bearer") {
            return Some(token.to_owned());
        }
        if scheme.eq_ignore_ascii_case("MediaBrowser") {
            return token.split(',').find_map(|field| {
                let (key, value) = field.trim().split_once('=')?;
                key.eq_ignore_ascii_case("Token")
                    .then(|| value.trim().trim_matches('"').to_owned())
            });
        }
    }
    None
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Credentials {
    pub username: String,
    pub pw: String,
}

pub async fn login(
    State(state): State<AppState>,
    Json(input): Json<Credentials>,
) -> Result<Json<Value>> {
    if input.username.len() > 128 || input.pw.len() > 1024 {
        return Err(Error::unauthorized());
    }
    let busy = || {
        Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many login attempts; try again later".into(),
        )
    };
    {
        let mut window = state.login_window.lock().map_err(Error::internal)?;
        if window.0.elapsed() >= Duration::from_secs(60) {
            *window = (std::time::Instant::now(), 0);
        }
        if window.1 >= 30 {
            return Err(busy());
        }
        window.1 += 1;
    }
    let permit = state
        .login_gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    let record = state
        .db
        .call(move |c| {
            Ok(c.query_row(
                "SELECT id,name,is_admin,password_hash FROM users WHERE name=?1",
                [input.username],
                |r| {
                    Ok((
                        User {
                            id: r.get(0)?,
                            name: r.get(1)?,
                            policy: Policy {
                                is_administrator: r.get(2)?,
                            },
                        },
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?)
        })
        .await?;
    let user = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if let Some((user, hash)) = record {
            let hash = PasswordHash::new(&hash).map_err(Error::internal)?;
            Argon2::default()
                .verify_password(input.pw.as_bytes(), &hash)
                .map_err(|_| Error::unauthorized())?;
            Ok(user)
        } else {
            // Do equivalent expensive work for unknown accounts to avoid a cheap username oracle.
            let _ = hash_password(&input.pw)?;
            Err(Error::unauthorized())
        }
    })
    .await
    .map_err(Error::internal)??;
    let token = format!("{}{}", id(), id());
    let hash = digest(&token);
    let user_id = user.id.clone();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM sessions WHERE expires_at<=?1", [now()])?;
            tx.execute(
                "INSERT INTO sessions VALUES (?1,?2,?3)",
                rusqlite::params![hash, user_id, now() + 30 * 86400],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(Json(
        json!({"User":user,"AccessToken":token,"ServerId":state.server_id}),
    ))
}
fn hash_password(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|p| p.to_string())
        .map_err(Error::internal)
}
pub async fn has_administrator(db: &Database) -> Result<bool> {
    db.call(|c| {
        Ok(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE is_admin=1)",
            [],
            |r| r.get(0),
        )?)
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SetupAdmin {
    pub name: String,
    pub password: String,
}

pub async fn setup(
    State(state): State<AppState>,
    Json(input): Json<SetupAdmin>,
) -> Result<(StatusCode, Json<User>)> {
    let _permit = state.login_gate.clone().try_acquire_owned().map_err(|_| {
        Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Password service busy".into(),
        )
    })?;
    if has_administrator(&state.db).await? {
        return Err(Error(
            StatusCode::CONFLICT,
            "Setup is already complete".into(),
        ));
    }
    let name = input.name.trim().to_owned();
    if name.is_empty()
        || name.len() > 128
        || input.password.len() < 12
        || input.password.len() > 1024
    {
        return Err(Error::bad(
            "Name must be 1–128 bytes; password must be 12–1024 bytes",
        ));
    }
    let hash = tokio::task::spawn_blocking(move || hash_password(&input.password))
        .await
        .map_err(Error::internal)??;
    let user = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let completed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE is_admin=1)",
                [],
                |r| r.get(0),
            )?;
            if completed {
                return Err(Error(
                    StatusCode::CONFLICT,
                    "Setup is already complete".into(),
                ));
            }
            let user = User {
                id: id(),
                name,
                policy: Policy {
                    is_administrator: true,
                },
            };
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO users VALUES (?1,?2,?3,1)",
                rusqlite::params![user.id, user.name, hash],
            )?;
            if inserted == 0 {
                return Err(Error(
                    StatusCode::CONFLICT,
                    "Username already exists".into(),
                ));
            }
            tx.commit()?;
            Ok(user)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(user)))
}

pub async fn add_user(db: &Database, name: String, password: String, admin: bool) -> Result<User> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.len() > 128 || password.len() < 12 || password.len() > 1024 {
        return Err(Error::bad(
            "Name must be 1–128 bytes; password must be 12–1024 bytes",
        ));
    }
    let hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(Error::internal)??;
    db.call(move |c| {
        let user = User {
            id: id(),
            name,
            policy: Policy {
                is_administrator: admin,
            },
        };
        let inserted = c.execute(
            "INSERT OR IGNORE INTO users VALUES (?1,?2,?3,?4)",
            rusqlite::params![user.id, user.name, hash, admin],
        )?;
        if inserted == 0 {
            return Err(Error(
                StatusCode::CONFLICT,
                "Username already exists".into(),
            ));
        }
        Ok(user)
    })
    .await
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NewUser {
    pub name: String,
    pub password: String,
    #[serde(default)]
    pub is_administrator: bool,
}
pub async fn create_user(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<NewUser>,
) -> Result<(StatusCode, Json<User>)> {
    auth.admin()?;
    let _permit = state.login_gate.clone().try_acquire_owned().map_err(|_| {
        Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Password service busy".into(),
        )
    })?;
    Ok((
        StatusCode::CREATED,
        Json(
            add_user(
                &state.db,
                input.name,
                input.password,
                input.is_administrator,
            )
            .await?,
        ),
    ))
}
pub async fn me(auth: Auth) -> Json<User> {
    Json(auth.user)
}
pub async fn users(auth: Auth, State(state): State<AppState>) -> Result<Json<Vec<User>>> {
    auth.admin()?;
    state
        .db
        .call(|c| {
            let mut statement = c.prepare("SELECT id,name,is_admin FROM users ORDER BY name")?;
            let result = statement
                .query_map([], |r| {
                    Ok(User {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        policy: Policy {
                            is_administrator: r.get(2)?,
                        },
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(Json(result))
        })
        .await
}
pub async fn logout(auth: Auth, State(state): State<AppState>) -> Result<StatusCode> {
    state
        .db
        .call(move |c| {
            c.execute(
                "DELETE FROM sessions WHERE token_hash=?1",
                [auth.token_hash],
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
