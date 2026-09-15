//! Connections to existing Jellyfin servers. Remote credentials never leave this backend.
use crate::{
    AppState,
    auth::{Auth, id, now},
    error::{Error, Result},
};
use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{Method, StatusCode, header},
    response::Response,
};
use futures_util::TryStreamExt;
use reqwest::Url;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub(crate) struct RemoteServer {
    pub id: String,
    pub base_url: String,
    pub user_id: String,
    pub token: String,
    pub device_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct RemoteServerView {
    id: String,
    name: String,
    url: String,
    server_id: String,
    last_sync: Option<i64>,
    last_error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConnectInput {
    name: String,
    url: String,
    username: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LoginResult {
    access_token: String,
    user: RemoteUser,
    server_id: Option<String>,
}
#[derive(Deserialize)]
struct RemoteUser {
    #[serde(rename = "Id")]
    id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PublicInfo {
    id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ItemPage {
    #[serde(default)]
    items: Vec<RemoteItem>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RemoteItem {
    id: String,
    name: String,
    #[serde(rename = "Type")]
    kind: String,
    collection_type: Option<String>,
    parent_id: Option<String>,
    container: Option<String>,
    size: Option<i64>,
    run_time_ticks: Option<i64>,
    #[serde(default)]
    media_streams: Vec<Value>,
    provider_ids: Option<Value>,
    production_year: Option<i64>,
    overview: Option<String>,
    #[serde(default)]
    genres: Vec<String>,
    community_rating: Option<f64>,
    index_number: Option<i64>,
    parent_index_number: Option<i64>,
}

fn upstream(message: impl std::fmt::Display) -> Error {
    tracing::warn!(error=%message, "Remote Jellyfin request failed");
    Error(
        StatusCode::BAD_GATEWAY,
        "The remote Jellyfin server is unavailable or rejected the request".into(),
    )
}
fn normalize_url(raw: &str) -> Result<String> {
    let mut url =
        Url::parse(raw.trim()).map_err(|_| Error::bad("Enter a valid Jellyfin server URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::bad(
            "The Jellyfin URL must be an HTTP(S) origin without credentials, query, or fragment",
        ));
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.to_string().trim_end_matches('/').to_owned())
}
fn local_id(source: &str, remote: &str) -> String {
    hex::encode(Sha256::digest(format!("{source}:{remote}").as_bytes()))[..32].to_owned()
}
fn endpoint(server: &RemoteServer, path: &str) -> Result<Url> {
    let base = format!("{}/", server.base_url.trim_end_matches('/'));
    Url::parse(&base)
        .and_then(|url| url.join(path.trim_start_matches('/')))
        .map_err(Error::internal)
}
fn authorized(builder: reqwest::RequestBuilder, server: &RemoteServer) -> reqwest::RequestBuilder {
    builder
        .header(header::USER_AGENT, "Jellymax/0.1")
        .header(
            "Authorization",
            format!(
                "MediaBrowser Client=\"Jellymax\", Device=\"Server\", DeviceId=\"{}\", Version=\"0.1\", Token=\"{}\"",
                server.device_id, server.token
            ),
        )
}
async fn checked(response: reqwest::Response, operation: &str) -> Result<reqwest::Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        let status = response.status();
        tracing::warn!(%status, %operation, "Remote Jellyfin rejected request");
        Err(Error(
            StatusCode::BAD_GATEWAY,
            format!("Remote Jellyfin returned {status} while {operation}"),
        ))
    }
}
async fn checked_login(response: reqwest::Response) -> Result<reqwest::Response> {
    match response.status() {
        status if status.is_success() => Ok(response),
        reqwest::StatusCode::UNAUTHORIZED => Err(Error(
            StatusCode::UNAUTHORIZED,
            "The remote Jellyfin username or password was rejected".into(),
        )),
        reqwest::StatusCode::FORBIDDEN => Err(Error(
            StatusCode::FORBIDDEN,
            "Jellyfin refused this user login. In that server's user settings, enable remote connections and check that its parental access schedule currently permits access".into(),
        )),
        status => {
            tracing::warn!(%status, "Remote Jellyfin rejected authentication");
            Err(Error(
                StatusCode::BAD_GATEWAY,
                format!("Remote Jellyfin returned {status} while authenticating"),
            ))
        }
    }
}
pub async fn list(
    auth: Auth,
    State(state): State<AppState>,
) -> Result<Json<Vec<RemoteServerView>>> {
    auth.admin()?;
    state.db.call(|c|{let mut q=c.prepare("SELECT id,name,base_url,server_id,last_sync,last_error FROM remote_servers ORDER BY name,id")?;
        Ok(Json(q.query_map([],|r|Ok(RemoteServerView{id:r.get(0)?,name:r.get(1)?,url:r.get(2)?,server_id:r.get(3)?,last_sync:r.get(4)?,last_error:r.get(5)?}))?.collect::<std::result::Result<Vec<_>,_>>()?))}).await
}
pub async fn connect(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<ConnectInput>,
) -> Result<(StatusCode, Json<Value>)> {
    auth.admin()?;
    if input.name.trim().is_empty()
        || input.name.len() > 128
        || input.username.len() > 128
        || input.password.len() > 1024
    {
        return Err(Error::bad("Invalid remote server details"));
    }
    let base_url = normalize_url(&input.url)?;
    let existing = state
        .db
        .call({
            let base_url = base_url.clone();
            move |c| {
                Ok(c.query_row(
                    "SELECT id,device_id FROM remote_servers WHERE base_url=?1",
                    [base_url],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?)
            }
        })
        .await?;
    let source_id = existing
        .as_ref()
        .map(|value| value.0.clone())
        .unwrap_or_else(id);
    let device_id = existing
        .map(|value| value.1)
        .unwrap_or_else(|| format!("jellymax-{source_id}"));
    let login:LoginResult=checked_login(state.http.post(format!("{base_url}/Users/AuthenticateByName"))
        .header("Authorization",format!("MediaBrowser Client=\"Jellymax\", Device=\"Server\", DeviceId=\"{device_id}\", Version=\"0.1\""))
        .json(&json!({"Username":input.username,"Pw":input.password})).send().await.map_err(upstream)?).await?.json().await.map_err(upstream)?;
    let server_id = match login.server_id {
        Some(v) => v,
        None => {
            checked(
                state
                    .http
                    .get(format!("{base_url}/System/Info/Public"))
                    .send()
                    .await
                    .map_err(upstream)?,
                "reading public server information",
            )
            .await?
            .json::<PublicInfo>()
            .await
            .map_err(upstream)?
            .id
        }
    };
    let source = RemoteServer {
        id: source_id,
        base_url,
        user_id: login.user.id,
        token: login.access_token,
        device_id,
    };
    let stored = source.clone();
    let name = input.name.trim().to_owned();
    let sid = server_id.clone();
    state.db.call(move|c|{c.execute("INSERT INTO remote_servers(id,name,base_url,server_id,user_id,access_token,device_id,last_error) VALUES (?1,?2,?3,?4,?5,?6,?7,NULL)
        ON CONFLICT(base_url) DO UPDATE SET name=excluded.name,server_id=excluded.server_id,user_id=excluded.user_id,access_token=excluded.access_token,device_id=excluded.device_id,last_error=NULL",
        params![stored.id,name,stored.base_url,sid,stored.user_id,stored.token,stored.device_id])?;Ok(())}).await?;
    if let Err(error) = sync_server(&state, &source).await {
        let source_id = source.id.clone();
        let message = error.to_string();
        state
            .db
            .call(move |c| {
                c.execute(
                    "UPDATE remote_servers SET last_error=?1 WHERE id=?2",
                    params![message, source_id],
                )?;
                Ok(())
            })
            .await?;
        return Err(error);
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({"Id":source.id,"ServerId":server_id})),
    ))
}
pub async fn sync(
    auth: Auth,
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<Value>> {
    auth.admin()?;
    let server = load_server(&state, &source_id).await?;
    let count = sync_server(&state, &server).await?;
    Ok(Json(json!({"Items":count})))
}
pub async fn remove(
    auth: Auth,
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<StatusCode> {
    auth.admin()?;
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "DELETE FROM libraries WHERE remote_server_id=?1",
                [&source_id],
            )?;
            if tx.execute("DELETE FROM remote_servers WHERE id=?1", [source_id])? == 0 {
                return Err(Error::missing());
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn load_server(state: &AppState, id: &str) -> Result<RemoteServer> {
    let id = id.to_owned();
    state
        .db
        .call(move |c| {
            c.query_row(
                "SELECT id,base_url,user_id,access_token,device_id FROM remote_servers WHERE id=?1",
                [id],
                |r| {
                    Ok(RemoteServer {
                        id: r.get(0)?,
                        base_url: r.get(1)?,
                        user_id: r.get(2)?,
                        token: r.get(3)?,
                        device_id: r.get(4)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(Error::missing)
        })
        .await
}
pub(crate) async fn for_item(
    state: &AppState,
    item: &str,
) -> Result<Option<(RemoteServer, String)>> {
    let item = item.to_owned();
    state.db.call(move|c|Ok(c.query_row("SELECT s.id,s.base_url,s.user_id,s.access_token,s.device_id,i.remote_item_id FROM items i JOIN remote_servers s ON s.id=i.remote_server_id WHERE i.id=?1",[item],|r|Ok((RemoteServer{id:r.get(0)?,base_url:r.get(1)?,user_id:r.get(2)?,token:r.get(3)?,device_id:r.get(4)?},r.get(5)?))).optional()?)).await
}

async fn sync_server(state: &AppState, server: &RemoteServer) -> Result<usize> {
    // Views is scoped to the connected user. Unlike Library/VirtualFolders it
    // does not require the remote account to be a server administrator.
    let mut views_url = endpoint(server, "UserViews")?;
    views_url
        .query_pairs_mut()
        .append_pair("IncludeExternalContent", "false");
    let views: ItemPage = checked(
        authorized(state.http.get(views_url), server)
            .send()
            .await
            .map_err(upstream)?,
        "reading the user's library views",
    )
    .await?
    .json()
    .await
    .map_err(upstream)?;
    let mut imports = Vec::new();
    for view in views.items {
        let remote_library = view.id.clone();
        let kind = match view.collection_type.as_deref() {
            Some("tvshows") => "tvshows",
            Some("music") => "music",
            Some("homevideos") => "homevideos",
            Some("movies") => "movies",
            _ => continue,
        }
        .to_owned();
        let mut url = endpoint(server, "Items")?;
        url.query_pairs_mut().append_pair("ParentId",&remote_library).append_pair("Recursive","true").append_pair("IncludeItemTypes","Movie,Series,Season,Episode,Audio").append_pair("Fields","MediaSources,MediaStreams,Overview,Genres,ProviderIds,ParentId,IndexNumber,ParentIndexNumber").append_pair("Limit","100000");
        let page: ItemPage = checked(
            authorized(state.http.get(url), server)
                .send()
                .await
                .map_err(upstream)?,
            "reading a remote library",
        )
        .await?
        .json()
        .await
        .map_err(upstream)?;
        imports.push((view, remote_library, kind, page.items));
    }
    let source = server.id.clone();
    let count = imports.iter().map(|v| v.3.len()).sum();
    let marker = id();
    state.db.call(move |c| {
        let tx = c.transaction()?;
        let active_libraries: Vec<String> = imports.iter().map(|v| local_id(&source, &v.1)).collect();
        for (folder, remote_library, kind, items) in &imports {
            let library = local_id(&source, remote_library);
            let item_ids: std::collections::HashSet<&str> =
                items.iter().map(|item| item.id.as_str()).collect();
            tx.execute("INSERT INTO libraries(id,name,path,kind,remote_server_id,remote_item_id) VALUES (?1,?2,?3,?4,?5,?6)
                ON CONFLICT(id) DO UPDATE SET name=excluded.name,kind=excluded.kind,remote_item_id=excluded.remote_item_id",
                params![library,folder.name,format!("remote://{source}/{remote_library}"),kind,source,remote_library])?;
            for item in items {
                let item_id=local_id(&source,&item.id);
                let tmdb=item.provider_ids.as_ref().and_then(|v|v.get("Tmdb")).and_then(Value::as_str);
                tx.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,tmdb_id,year,overview,genres,rating,index_number,parent_index_number,parent_id,scan_id,remote_server_id,remote_item_id)
                    VALUES (?1,?2,?3,?4,?5,?6,?7,strftime('%s','now'),?8,?9,?10,?11,?12,?13,?14,?15,?16,NULL,?17,?18,?19)
                    ON CONFLICT(id) DO UPDATE SET library_id=excluded.library_id,name=excluded.name,kind=excluded.kind,container=excluded.container,size=excluded.size,modified=CASE WHEN items.modified=0 THEN excluded.modified ELSE items.modified END,runtime_ticks=excluded.runtime_ticks,media_streams=excluded.media_streams,tmdb_id=excluded.tmdb_id,year=excluded.year,overview=excluded.overview,genres=excluded.genres,rating=excluded.rating,index_number=excluded.index_number,parent_index_number=excluded.parent_index_number,parent_id=NULL,scan_id=excluded.scan_id,remote_item_id=excluded.remote_item_id",
                    params![item_id,library,format!("remote://{source}/{}",item.id),item.name,item.kind,item.container.clone().unwrap_or_default(),item.size.unwrap_or(0),item.run_time_ticks,serde_json::to_string(&item.media_streams).map_err(Error::internal)?,tmdb,item.production_year,item.overview,serde_json::to_string(&item.genres).map_err(Error::internal)?,item.community_rating,item.index_number,item.parent_index_number,marker,source,item.id])?;
            }
            for item in items {
                if let Some(parent) = item
                    .parent_id
                    .as_ref()
                    .filter(|parent| item_ids.contains(parent.as_str()))
                {
                    tx.execute("UPDATE items SET parent_id=?1 WHERE id=?2 AND library_id=?3",params![local_id(&source,parent),local_id(&source,&item.id),library])?;
                }
            }
        }
        tx.execute("DELETE FROM items WHERE remote_server_id=?1 AND scan_id<>?2",params![source,marker])?;
        let stale: Vec<String> = {
            let mut statement=tx.prepare("SELECT id FROM libraries WHERE remote_server_id=?1")?;
            statement.query_map([&source],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?
        };
        for library in stale { if !active_libraries.contains(&library) { tx.execute("DELETE FROM libraries WHERE id=?1",[library])?; } }
        tx.execute("UPDATE remote_servers SET last_sync=?1,last_error=NULL WHERE id=?2",params![now(),source])?;
        tx.commit()?; Ok(())
    }).await?;
    Ok(count)
}

// Read the original bytes on the remote server. Static streaming must be used
// here so only this Rust server can run FFmpeg for remote items.
pub(crate) async fn ffmpeg_source(
    state: &AppState,
    item: &str,
) -> Result<Option<(String, Option<String>)>> {
    let Some((server, remote)) = for_item(state, item).await? else {
        return Ok(None);
    };
    let route = state
        .db
        .call({
            let item = item.to_owned();
            move |c| {
                Ok(
                    c.query_row("SELECT kind FROM items WHERE id=?1", [item], |r| {
                        r.get::<_, String>(0)
                    })?,
                )
            }
        })
        .await?;
    let url = static_stream_url(&server, &remote, &route)?;
    let headers = format!(
        "Authorization: MediaBrowser Client=\"Jellymax\", Device=\"Server\", DeviceId=\"{}\", Version=\"0.1\", Token=\"{}\"\r\n",
        server.device_id, server.token
    );
    Ok(Some((url.to_string(), Some(headers))))
}

/// Use the media source that advertised this embedded subtitle. Jellyfin may
/// default its static stream endpoint to a different source for multi-version
/// items, where the same FFmpeg stream index means something else.
pub(crate) async fn embedded_subtitle_source(
    state: &AppState,
    item: &str,
    index: usize,
    codec: &str,
) -> Result<Option<(String, Option<String>, usize)>> {
    let Some((server, remote)) = for_item(state, item).await? else {
        return Ok(None);
    };
    let mut info_url = endpoint(&server, &format!("Items/{remote}/PlaybackInfo"))?;
    info_url
        .query_pairs_mut()
        .append_pair("UserId", &server.user_id);
    let info: Value = checked(
        authorized(state.http.get(info_url), &server)
            .send()
            .await
            .map_err(upstream)?,
        "reading remote subtitle sources",
    )
    .await?
    .json()
    .await
    .map_err(upstream)?;
    let source = info["MediaSources"]
        .as_array()
        .and_then(|sources| {
            sources.iter().find(|source| {
                source["MediaStreams"].as_array().is_some_and(|streams| {
                    streams.iter().any(|stream| {
                        stream["Type"] == "Subtitle"
                            && stream["Index"].as_u64() == Some(index as u64)
                            && stream["IsExternal"] != true
                            && stream["Codec"].as_str().is_some_and(|value| value.eq_ignore_ascii_case(codec))
                    })
                })
            })
        })
        .ok_or_else(|| Error::bad("The selected subtitle is no longer present on the remote media source; sync the server again"))?;
    let source_id = source["Id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::bad("The remote media source has no identifier"))?;
    let ordinal = source["MediaStreams"]
        .as_array()
        .and_then(|streams| {
            streams
                .iter()
                .filter(|stream| stream["Type"] == "Subtitle" && stream["IsExternal"] != true)
                .position(|stream| stream["Index"].as_u64() == Some(index as u64))
        })
        .ok_or_else(Error::missing)?;
    let route = state
        .db
        .call({
            let item = item.to_owned();
            move |c| {
                Ok(
                    c.query_row("SELECT kind FROM items WHERE id=?1", [item], |r| {
                        r.get::<_, String>(0)
                    })?,
                )
            }
        })
        .await?;
    let mut url = static_stream_url(&server, &remote, &route)?;
    url.query_pairs_mut()
        .append_pair("MediaSourceId", source_id);
    let headers = format!(
        "Authorization: MediaBrowser Client=\"Jellymax\", Device=\"Server\", DeviceId=\"{}\", Version=\"0.1\", Token=\"{}\"\r\n",
        server.device_id, server.token
    );
    Ok(Some((url.to_string(), Some(headers), ordinal)))
}

// Fetch an external subtitle in its original text format. Requesting the same
// format avoids Jellyfin's subtitle conversion/cache path on the media server.
pub(crate) async fn external_subtitle(
    state: &AppState,
    item: &str,
    index: usize,
    codec: &str,
) -> Result<Vec<u8>> {
    let (server, remote) = for_item(state, item).await?.ok_or_else(Error::missing)?;
    if !remote
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(Error::bad("Invalid remote media identifier"));
    }
    let format = match codec.to_ascii_lowercase().as_str() {
        "subrip" | "srt" => "srt",
        "webvtt" | "vtt" => "vtt",
        _ => return Err(Error::missing()),
    };
    let mut info_url = endpoint(&server, &format!("Items/{remote}/PlaybackInfo"))?;
    info_url
        .query_pairs_mut()
        .append_pair("UserId", &server.user_id);
    let info: Value = checked(
        authorized(state.http.get(info_url), &server)
            .send()
            .await
            .map_err(upstream)?,
        "reading remote subtitle sources",
    )
    .await?
    .json()
    .await
    .map_err(upstream)?;
    let media_source_id = info["MediaSources"]
        .as_array()
        .and_then(|sources| {
            sources.iter().find(|source| {
                source["MediaStreams"].as_array().is_some_and(|streams| {
                    streams.iter().any(|stream| {
                        stream["Type"] == "Subtitle"
                            && stream["Index"].as_u64() == Some(index as u64)
                            && stream["IsExternal"] == true
                    })
                })
            })
        })
        .and_then(|source| source["Id"].as_str())
        .ok_or_else(Error::missing)?;
    if !media_source_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(Error::bad("Invalid remote media source identifier"));
    }
    let url = endpoint(
        &server,
        &format!("Videos/{remote}/{media_source_id}/Subtitles/{index}/Stream.{format}"),
    )?;
    let mut response = checked(
        authorized(state.http.get(url), &server)
            .send()
            .await
            .map_err(upstream)?,
        "reading remote subtitle",
    )
    .await?;
    const MAX_SUBTITLE: usize = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(upstream)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_SUBTITLE {
            return Err(Error::bad("Subtitle track is too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn static_stream_url(server: &RemoteServer, remote: &str, kind: &str) -> Result<Url> {
    let route = if kind == "Audio" { "Audio" } else { "Videos" };
    let mut url = endpoint(server, &format!("{route}/{remote}/stream"))?;
    url.query_pairs_mut().append_pair("static", "true");
    Ok(url)
}

pub async fn stream(
    State(state): State<AppState>,
    Path(item): Path<String>,
    request: axum::extract::Request,
) -> Result<Response> {
    let request = crate::tickets::authorize(&state, &item, request).await?;
    let (server, remote) = for_item(&state, &item).await?.ok_or_else(Error::missing)?;
    let is_audio = state
        .db
        .call({
            let item = item.clone();
            move |c| {
                Ok(
                    c.query_row("SELECT kind='Audio' FROM items WHERE id=?1", [item], |r| {
                        r.get::<_, bool>(0)
                    })?,
                )
            }
        })
        .await?;
    let url = static_stream_url(&server, &remote, if is_audio { "Audio" } else { "Movie" })?;
    let mut outbound = authorized(
        state.http.request(
            if request.method() == Method::HEAD {
                reqwest::Method::HEAD
            } else {
                reqwest::Method::GET
            },
            url,
        ),
        &server,
    );
    for name in [header::RANGE, header::IF_RANGE] {
        if let Some(value) = request.headers().get(&name) {
            outbound = outbound.header(name, value)
        }
    }
    let response = outbound.send().await.map_err(upstream)?;
    let status = response.status();
    let headers = response.headers().clone();
    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let mut output = Response::builder().status(status);
    for name in [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
        header::ETAG,
        header::LAST_MODIFIED,
    ] {
        if let Some(value) = headers.get(&name) {
            output = output.header(name, value)
        }
    }
    output
        .body(Body::from_stream(stream))
        .map_err(Error::internal)
}
pub(crate) async fn image(state: &AppState, item: &str) -> Result<Option<Response>> {
    let Some((server, remote)) = for_item(state, item).await? else {
        return Ok(None);
    };
    let directory = state.data_dir.join("remote-artwork");
    // Version the file name so existing 480px/quality-85 thumbnails are not reused.
    let cached = directory.join(format!("{item}.hq.jpg"));
    if let Ok(metadata) = tokio::fs::metadata(&cached).await
        && metadata.is_file()
        && metadata
            .modified()
            .ok()
            .and_then(|time| time.elapsed().ok())
            .is_some_and(|age| age < std::time::Duration::from_secs(86_400))
    {
        let bytes = tokio::fs::read(cached).await?;
        return Response::builder()
            .header(header::CONTENT_TYPE, "image/jpeg")
            .body(Body::from(bytes))
            .map(Some)
            .map_err(Error::internal);
    }
    let mut url = endpoint(&server, &format!("Items/{remote}/Images/Primary"))?;
    url.query_pairs_mut()
        .append_pair("MaxWidth", "1200")
        .append_pair("Quality", "95")
        .append_pair("Format", "Jpg");
    let response = authorized(state.http.get(url), &server)
        .send()
        .await
        .map_err(upstream)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(Error::missing());
    }
    let response = checked(response, "reading remote artwork").await?;
    if response
        .content_length()
        .is_some_and(|length| length > 4 * 1024 * 1024)
    {
        return Err(upstream("remote artwork exceeded the thumbnail size limit"));
    }
    let bytes = response.bytes().await.map_err(upstream)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(upstream("remote artwork exceeded the thumbnail size limit"));
    }
    tokio::fs::create_dir_all(&directory).await?;
    tokio::fs::write(cached, &bytes).await?;
    prune_artwork(&directory).await;
    Ok(Some(
        Response::builder()
            .header(header::CONTENT_TYPE, "image/jpeg")
            .body(Body::from(bytes))
            .map_err(Error::internal)?,
    ))
}

async fn prune_artwork(directory: &std::path::Path) {
    const MAX_FILES: usize = 2_000;
    const MAX_BYTES: u64 = 512 * 1024 * 1024;
    let Ok(mut entries) = tokio::fs::read_dir(directory).await else {
        return;
    };
    let mut files = Vec::new();
    let mut total_bytes = 0_u64;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.path().extension().and_then(|value| value.to_str()) == Some("jpg")
            && let Ok(metadata) = entry.metadata().await
        {
            total_bytes = total_bytes.saturating_add(metadata.len());
            files.push((metadata.modified().ok(), metadata.len(), entry.path()));
        }
    }
    if files.len() <= MAX_FILES && total_bytes <= MAX_BYTES {
        return;
    }
    files.sort_by_key(|entry| entry.0);
    let mut remaining = files.len();
    for (_, size, path) in files {
        if remaining <= MAX_FILES && total_bytes <= MAX_BYTES {
            break;
        }
        let _ = tokio::fs::remove_file(path).await;
        remaining = remaining.saturating_sub(1);
        total_bytes = total_bytes.saturating_sub(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_normalizes_remote_origins() {
        assert_eq!(
            normalize_url(" https://media.example/jellyfin/ ").unwrap(),
            "https://media.example/jellyfin"
        );
        for invalid in [
            "file:///tmp/media",
            "https://user:pw@example.com",
            "https://example.com?q=token",
            "not a url",
        ] {
            assert!(normalize_url(invalid).is_err());
        }
    }

    #[test]
    fn remote_ids_are_stable_and_namespaced() {
        assert_eq!(local_id("one", "item"), local_id("one", "item"));
        assert_ne!(local_id("one", "item"), local_id("two", "item"));
        assert_eq!(local_id("one", "item").len(), 32);
    }

    #[test]
    fn remote_stream_url_is_always_static() {
        let server = RemoteServer {
            id: "id".into(),
            base_url: "https://example.com/jellyfin".into(),
            user_id: "user".into(),
            token: "secret".into(),
            device_id: "device".into(),
        };
        assert_eq!(
            static_stream_url(&server, "movie", "Movie")
                .unwrap()
                .as_str(),
            "https://example.com/jellyfin/Videos/movie/stream?static=true"
        );
        assert_eq!(
            static_stream_url(&server, "song", "Audio")
                .unwrap()
                .as_str(),
            "https://example.com/jellyfin/Audio/song/stream?static=true"
        );
    }
}
