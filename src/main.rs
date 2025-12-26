use anyhow::Context;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::StatusCode,
    response::{sse, Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use directories::ProjectDirs;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    process::Command,
    signal,
    sync::broadcast,
};
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
use tower_http::services::ServeDir;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    project_dirs: ProjectDirs,
    config: Config,
    event_channel: broadcast::Sender<ServerEvent>,
    // This is essentially a global lock, meaning that only one request can be handled at a time.
    // Considering the small number of users this is fine and it keeps the code simple.
    processes: Arc<tokio::sync::Mutex<HashMap<Uuid, ServerProcess>>>,
}

struct ServerProcess {
    tcp_stream: tokio::net::TcpStream,
    server_path: PathBuf,
    map_path: PathBuf,
    port: u16,
}

#[derive(Clone)]
struct ServerEvent {
    server_id: Uuid,
    event: String,
    data: String,
}

#[derive(Clone, Deserialize)]
struct Config {
    http_port: u16,
    executable_path: PathBuf,
    port_range: (u16, u16),
    public_address: String,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let project_dirs = ProjectDirs::from("org", "ddnet", "trashmap")
        .context("Could not determine the user's home directory")?;
    let config_path = project_dirs.config_dir().join("config.toml");
    let config = tokio::fs::read_to_string(&config_path)
        .await
        .with_context(|| format!("Failed to read config from {}", config_path.display()))?;
    let config: Config = toml::from_str(&config)
        .with_context(|| format!("Failed to parse config file at {}", config_path.display()))?;

    let state = AppState {
        project_dirs: project_dirs,
        config: config.clone(),
        event_channel: broadcast::Sender::new(100),
        processes: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/server-events", get(server_events))
        .route("/update-settings", get(update_settings))
        .route(
            "/update-map",
            post(update_map).layer(
                DefaultBodyLimit::max(10_000_000), // Set map upload limit to 10 MB
            ),
        )
        .with_state(state.clone());

    let app = if cfg!(debug_assertions) {
        app.fallback_service(ServeDir::new("www"))
    } else {
        app.route("/", get(Html(include_str!("../www/index.html"))))
            .route(
                "/script.js",
                get((
                    [("content-type", "application/javascript")],
                    include_str!("../www/script.js"),
                )),
            )
    };

    tokio::spawn(log_errors(async move {
        let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }

        let mut processes = state.processes.lock().await;
        for process in processes.values_mut() {
            process.tcp_stream.write_all(b"shutdown\n").await?;

            tokio::fs::remove_dir_all(&process.server_path).await?;
        }

        std::process::exit(0);
    }));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", config.http_port)).await?;
    println!("Listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Deserialize)]
struct ServerEventsQuery {
    server_id: Uuid,
}

async fn server_events(
    State(state): State<AppState>,
    Query(query): Query<ServerEventsQuery>,
) -> (
    [(&'static str, &'static str); 1],
    sse::Sse<impl Stream<Item = Result<sse::Event, std::convert::Infallible>>>,
) {
    let stream = BroadcastStream::new(state.event_channel.subscribe())
        .filter_map(|r| r.ok()) // Silently skip messages if lagging behind
        .filter(move |m| m.server_id == query.server_id)
        .map(|m| sse::Event::default().event(m.event).data(m.data))
        .map(Ok);

    let processes = state.processes.lock().await;
    let status = match processes.get(&query.server_id) {
        Some(process) => sse::Event::default()
            .event("online")
            .data(format!("{}:{}", state.config.public_address, process.port)),

        None => sse::Event::default().event("offline"),
    };

    (
        [("x-accel-buffering", "no")], // Tell the nginx reverse proxy to disable buffering
        sse::Sse::new(tokio_stream::once(Ok(status)).chain(stream))
            .keep_alive(sse::KeepAlive::new()), // Prevent timeout in the nginx reverse proxy
    )
}

fn escape_ddnet(str: &str) -> String {
    str.replace(r"\", r"\\").replace("\"", "\\\"").replace("\n", "").replace("\r", "")
}

#[derive(Deserialize)]
struct UpdateSettingsQuery {
    server_id: Uuid,
    server_name: String,
    server_password: String,
}

async fn update_settings(
    State(state): State<AppState>,
    Query(query): Query<UpdateSettingsQuery>,
) -> Result<StatusCode, AppError> {
    let mut processes = state.processes.lock().await;
    if let Some(process) = processes.get_mut(&query.server_id) {
        process
            .tcp_stream
            .write_all(
                [
                    format!("sv_name \"{}\"\n", escape_ddnet(&query.server_name)),
                    format!("password \"{}\"\n", escape_ddnet(&query.server_password)),
                ]
                .concat()
                .as_bytes(),
            )
            .await?;
        Ok(StatusCode::ACCEPTED)
    } else {
        Ok(StatusCode::OK)
    }
}

#[derive(Deserialize)]
struct UpdateMapQuery {
    server_id: Uuid,
    map_filename: String,
    server_name: String,
    server_password: String,
}

async fn update_map(
    State(state): State<AppState>,
    Query(query): Query<UpdateMapQuery>,
    map_bytes: Bytes,
) -> Result<StatusCode, AppError> {
    let map_filename = Path::new(&query.map_filename)
        .file_name()
        .context("Not a valid filename")?
        .to_str()
        .unwrap(); // Never panics because the path wraps a valid string
    let map_name = map_filename
        .strip_suffix(".map")
        .context("The filename must end with the .map extension")?;
    let server_path = state
        .project_dirs
        .data_dir()
        .join(query.server_id.to_string());
    let map_path = server_path.join("maps").join(map_filename);

    let mut processes = state.processes.lock().await;
    if let Some(process) = processes.get_mut(&query.server_id) {
        tokio::fs::write(&map_path, map_bytes).await?;

        if process.map_path == map_path {
            process.tcp_stream.write_all(b"hot_reload\n").await?;
        } else {
            process
                .tcp_stream
                .write_all(format!("change_map \"{}\"\n", escape_ddnet(map_name)).as_bytes())
                .await?;
            tokio::fs::remove_file(&process.map_path).await?;
            process.map_path = map_path;
        }

        Ok(StatusCode::ACCEPTED)
    } else {
        let occupied_ports: HashSet<u16> = processes.values().map(|p| p.port).collect();
        let port = (state.config.port_range.0..=state.config.port_range.1)
            .filter(|p| !occupied_ports.contains(p))
            .next()
            .context("Could not start the server because all ports are occupied")?;

        tokio::fs::create_dir_all(server_path.join("maps")).await?;
        tokio::fs::write(&map_path, map_bytes).await?;

        tokio::fs::write(server_path.join("storage.cfg"), "add_path $CURRENTDIR\n").await?;
        tokio::fs::write(
            server_path.join("autoexec.cfg"),
            [
                &format!("sv_port {port}\n"),
                &format!("sv_map \"{}\"\n", escape_ddnet(map_name)),
                &format!("sv_name \"{}\"\n", escape_ddnet(&query.server_name)),
                &format!("password \"{}\"\n", escape_ddnet(&query.server_password)),
                &format!("ec_port {port}\n"),
                "ec_bindaddr \"127.0.0.1\"\n",
                "ec_password \"open sesame\"\n",
                "ec_output_level -3\n", // Prevent the TCP buffer running full
                "sv_motd \"Use rcon password \\\"test\\\" or /practice for testing. Instead of \\\"super\\\" use \\\"invincible\\\" to toggle invincibility.\"\n",
                "sv_test_cmds 1\n",
                "sv_rescue 1\n",
                "sv_rcon_helper_password \"test\"\n",
                "sv_tele_others_auth_level 3\n", // Forbid teleporting other players
                "access_level totele 2\n",
                "access_level totelecp 2\n",
                "access_level tele 2\n",
                "access_level addweapon 2\n",
                "access_level removeweapon 2\n",
                "access_level shotgun 2\n",
                "access_level grenade 2\n",
                "access_level laser 2\n",
                "access_level rifle 2\n",
                "access_level jetpack 2\n",
                "access_level setjumps 2\n",
                "access_level weapons 2\n",
                "access_level unshotgun 2\n",
                "access_level ungrenade 2\n",
                "access_level unlaser 2\n",
                "access_level unrifle 2\n",
                "access_level unjetpack 2\n",
                "access_level unweapons 2\n",
                "access_level ninja 2\n",
                "access_level unninja 2\n",
                "access_level invincible 2\n",
                "access_level endless_hook 2\n",
                "access_level unendless_hook 2\n",
                "access_level solo 2\n",
                "access_level unsolo 2\n",
                "access_level freeze 2\n",
                "access_level unfreeze 2\n",
                "access_level deep 2\n",
                "access_level undeep 2\n",
                "access_level livefreeze 2\n",
                "access_level unlivefreeze 2\n",
                "access_level left 2\n",
                "access_level right 2\n",
                "access_level up 2\n",
                "access_level down 2\n",
                "access_level move 2\n",
                "access_level move_raw 2\n",
            ]
            .concat(),
        )
        .await?;

        let mut child = Command::new(&state.config.executable_path)
            .current_dir(&server_path)
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().unwrap();

        let state_clone = state.clone();
        let server_path_clone = server_path.clone();
        tokio::task::spawn(log_errors(async move {
            child.wait().await?;

            let mut processes = state_clone.processes.lock().await;
            if processes.remove(&query.server_id).is_some() {
                let _ = state_clone.event_channel.send(ServerEvent {
                    server_id: query.server_id,
                    event: "stopped".to_owned(),
                    data: String::new(),
                });
            }

            tokio::fs::remove_dir_all(server_path_clone).await?;

            Ok(())
        }));

        let mut lines = tokio::io::BufReader::new(stdout).lines();
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(1);
        loop {
            let line = tokio::time::timeout_at(deadline, lines.next_line())
                .await??
                .context("The server process stopped unexpectedly")?;
            if line.contains("econ: bound to 127.0.0.1") {
                break;
            }
        }

        let mut tcp_stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;

        tcp_stream.write_all(b"open sesame\n").await?;
        tcp_stream.write_all(b"stdout_output_level -3\n").await?; // Prevent the pipe running full

        processes.insert(
            query.server_id,
            ServerProcess {
                tcp_stream,
                server_path,
                map_path,
                port,
            },
        );

        let _ = state.event_channel.send(ServerEvent {
            server_id: query.server_id,
            event: "online".to_owned(),
            data: format!("{}:{}", state.config.public_address, port),
        });

        let state_clone = state.clone();
        tokio::task::spawn(log_errors(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

            let mut processes = state_clone.processes.lock().await;
            if let Some(process) = processes.get_mut(&query.server_id) {
                process
                    .tcp_stream
                    .write_all(b"sv_shutdown_when_empty 1\n")
                    .await?;

                let _ = state_clone.event_channel.send(ServerEvent {
                    server_id: query.server_id,
                    event: "shutdownwhenempty".to_owned(),
                    data: String::new(),
                });
            }

            Ok(())
        }));

        Ok(StatusCode::CREATED)
    }
}

async fn log_errors(future: impl std::future::Future<Output = Result<(), anyhow::Error>>) {
    if let Err(error) = future.await {
        eprintln!("Error in task: {error:?}");
    }
}

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        eprintln!("Error in handler: {:?}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        Self(error.into())
    }
}
