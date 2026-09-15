//! Short-lived, item-scoped credentials for browser media elements, which cannot add auth headers.
use crate::{
    AppState,
    auth::{Auth, now},
    error::{Error, Result},
};
use axum::extract::{FromRequestParts, Query, Request};
use rand_core::{OsRng, RngCore};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const LIFETIME_SECONDS: i64 = 4 * 3600;
const MAX_SESSION_TICKETS: i64 = 32;

fn digest(ticket: &str) -> String {
    hex::encode(Sha256::digest(ticket.as_bytes()))
}

pub(crate) async fn issue(state: &AppState, auth: &Auth, item: &str) -> Result<String> {
    let mut random = [0u8; 32];
    OsRng.fill_bytes(&mut random);
    let ticket = hex::encode(random);
    let ticket_hash = digest(&ticket);
    let session_hash = auth.token_hash.clone();
    let user = auth.user.id.clone();
    let item = item.to_owned();
    state
        .db
        .call(move |c| {
            let timestamp = now();
            let tx = c.transaction()?;
            let session_expiry: i64 = tx
                .query_row(
                    "SELECT expires_at FROM sessions WHERE token_hash=?1 AND user_id=?2 AND expires_at>?3",
                    params![session_hash, user, timestamp],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(Error::unauthorized)?;
            if !tx.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE id=?1)", [&item], |r| r.get::<_, bool>(0))? {
                return Err(Error::missing());
            }
            tx.execute("DELETE FROM playback_tickets WHERE expires_at<=?1", [timestamp])?;
            // Keep space for this ticket and bound storage even when clients repeatedly request PlaybackInfo.
            tx.execute(
                "DELETE FROM playback_tickets WHERE session_hash=?1 AND ticket_hash NOT IN
                    (SELECT ticket_hash FROM playback_tickets WHERE session_hash=?1
                     ORDER BY created_at DESC,rowid DESC LIMIT ?2)",
                params![session_hash, MAX_SESSION_TICKETS - 1],
            )?;
            tx.execute(
                "INSERT INTO playback_tickets(ticket_hash,session_hash,item_id,expires_at,created_at)
                 VALUES (?1,?2,?3,?4,?5)",
                params![ticket_hash, session_hash, item, session_expiry.min(timestamp + LIFETIME_SECONDS), timestamp],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(ticket)
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TicketQuery {
    playback_ticket: Option<String>,
}

/// Use only on the media stream/download and external subtitle delivery handlers.
/// A ticket is deliberately not accepted by the general account authentication extractor.
pub(crate) async fn authorize(state: &AppState, item: &str, request: Request) -> Result<Request> {
    let (mut parts, body) = request.into_parts();
    if ["X-Emby-Token", "Authorization", "X-Emby-Authorization"]
        .iter()
        .any(|header| parts.headers.contains_key(*header))
    {
        // If account credentials are supplied, invalid credentials must not silently fall back to a ticket.
        Auth::from_request_parts(&mut parts, state).await?;
    } else {
        let Query(query) =
            Query::<TicketQuery>::try_from_uri(&parts.uri).map_err(|_| Error::unauthorized())?;
        let ticket = query.playback_ticket.ok_or_else(Error::unauthorized)?;
        if ticket.len() != 64 || !ticket.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::unauthorized());
        }
        let hash = digest(&ticket);
        let item = item.to_owned();
        state
            .db
            .call(move |c| {
                let valid: bool = c.query_row(
                    "SELECT EXISTS(SELECT 1 FROM playback_tickets t
                     JOIN sessions s ON s.token_hash=t.session_hash
                     WHERE t.ticket_hash=?1 AND t.item_id=?2 AND t.expires_at>?3 AND s.expires_at>?3)",
                    params![hash, item, now()],
                    |r| r.get(0),
                )?;
                if !valid {
                    return Err(Error::unauthorized());
                }
                Ok(())
            })
            .await?;
    }
    Ok(Request::from_parts(parts, body))
}
