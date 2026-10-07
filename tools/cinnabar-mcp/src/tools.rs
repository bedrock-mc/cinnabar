//! Tool definitions and dispatch.

use std::{
    env::consts::EXE_SUFFIX,
    path::{Path, PathBuf},
    time::Duration,
};

use developer_control::{
    ENDPOINT_DIR, ENDPOINT_ENV, HIDDEN_WINDOW_ENV, WINDOW_SIZE_ENV, client::Controller,
    endpoint::Endpoint, protocol::Command,
};
use mcp_stdio::{png_content, text_content};
use serde_json::{Value, json};

use crate::{
    commands::{self, CALL_TIMEOUT, is_loopback, resolve},
    processes::{Client, Launch, LocalServer},
};

const DEFAULT_SIZE: [u32; 2] = [1920, 1080];
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(180);
/// Inline screenshots are downscaled to this width to stay small in a model's context.
const DEFAULT_INLINE_WIDTH: u32 = 1280;
const CLIENT_BINARY: &str = "target/debug/bedrock-client";
const LOCAL_SERVER_BINARY: &str = "target/release/bedrock-local-server";
const SHOWCASE_WORLD: &str = "showcase-world";

pub struct Server {
    repo: PathBuf,
    control_dir: PathBuf,
    client: Option<Client>,
    controller: Option<Controller>,
    /// Where `controller` connected, for replacing a broken connection.
    endpoint: Option<Endpoint>,
    local_server: Option<LocalServer>,
}

impl mcp_stdio::ToolServer for Server {
    fn info(&self) -> (&'static str, &'static str) {
        ("cinnabar-mcp", env!("CARGO_PKG_VERSION"))
    }

    fn definitions(&self) -> Value {
        crate::definitions::definitions()
    }

    fn call(&mut self, name: &str, arguments: &Value) -> Value {
        if name == "screenshot" {
            return self.screenshot(arguments);
        }
        let outcome = match name {
            "launch_client" => self.launch(arguments),
            "connect" => self.connect(arguments),
            "quit" => Ok(self.quit()),
            _ => commands::command(name, arguments, &self.repo, &self.control_dir)
                .and_then(|(command, timeout)| self.send(&command, timeout)),
        };
        match outcome {
            Ok(value) => text_content(&value, false),
            Err(message) => text_content(&json!({ "error": message }), true),
        }
    }
}

impl Server {
    pub fn new(repo: PathBuf) -> Self {
        let control_dir = repo.join(".local").join(ENDPOINT_DIR);
        Self {
            repo,
            control_dir,
            client: None,
            controller: None,
            endpoint: None,
            local_server: None,
        }
    }

    /// Drives an already running client through its endpoint file.
    pub fn attach(&mut self, endpoint: &Endpoint) -> Result<(), String> {
        let controller = Controller::connect_to(endpoint)
            .map_err(|error| format!("connect to the client's control endpoint: {error}"))?;
        self.controller = Some(controller);
        self.endpoint = Some(endpoint.clone());
        Ok(())
    }

    pub(crate) fn send(&mut self, command: &Command, timeout: Duration) -> Result<Value, String> {
        if let Some(client) = self.client.as_mut()
            && !client.running()
        {
            let log = client.log.display().to_string();
            self.client = None;
            self.controller = None;
            return Err(format!("the client has exited; see {log}"));
        }
        let controller = self
            .controller
            .as_mut()
            .ok_or("no client is running; call launch_client first")?;
        let outcome = controller.call(command, timeout);
        // A late reply would answer the next request; start a fresh connection instead.
        if controller.is_broken()
            && let Some(endpoint) = &self.endpoint
        {
            self.controller = Controller::connect_to(endpoint).ok();
        }
        outcome
    }

    fn launch(&mut self, arguments: &Value) -> Result<Value, String> {
        if self.controller.is_some() {
            return Err("a client is already running; call quit first".into());
        }
        let binary = arguments.get("binary").and_then(Value::as_str).map_or_else(
            || executable(&self.repo, CLIENT_BINARY, EXE_SUFFIX),
            |path| resolve(&self.repo, path),
        );
        let size = match arguments.get("size") {
            Some(value) => serde_json::from_value::<[u32; 2]>(value.clone())
                .map_err(|_| "`size` must be [width, height]")?,
            None => DEFAULT_SIZE,
        };
        let headless = arguments.get("headless").and_then(Value::as_bool) == Some(true);
        let args: Vec<String> = match arguments.get("args") {
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| "`args` must be an array of strings")?,
            None => Vec::new(),
        };
        let endpoint = self
            .control_dir
            .join(format!("endpoint-{}.json", std::process::id()));
        let mut env = vec![
            (ENDPOINT_ENV.to_owned(), endpoint.display().to_string()),
            (
                WINDOW_SIZE_ENV.to_owned(),
                format!("{}x{}", size[0], size[1]),
            ),
        ];
        if headless {
            env.push((HIDDEN_WINDOW_ENV.to_owned(), "1".to_owned()));
        }
        if let Some(extra) = arguments.get("env").and_then(Value::as_object) {
            for (key, value) in extra {
                let value = value.as_str().ok_or("`env` values must be strings")?;
                env.push((key.clone(), value.to_owned()));
            }
        }
        let timeout = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .map_or(LAUNCH_TIMEOUT, Duration::from_millis);
        let log = commands::generated(&self.control_dir, "logs", "client.log");
        let (client, published) = Client::launch(Launch {
            binary: &binary,
            args: &args,
            env: &env,
            endpoint: &endpoint,
            log,
            timeout,
        })?;
        let (pid, log) = (client.pid(), client.log.clone());
        self.client = Some(client);
        if let Err(error) = self.attach(&published) {
            self.quit();
            return Err(error);
        }
        Ok(json!({ "pid": pid, "size": size, "headless": headless, "log": log }))
    }

    fn connect(&mut self, arguments: &Value) -> Result<Value, String> {
        let address = if let Some(local) = arguments.get("local_server") {
            self.start_local_server(local)?
        } else {
            let address = arguments
                .get("address")
                .and_then(Value::as_str)
                .ok_or("give `address` or `local_server`")?;
            let allow_remote = arguments.get("allow_remote").and_then(Value::as_bool) == Some(true);
            if !is_loopback(address) && !allow_remote {
                return Err(format!(
                    "{address} is not a loopback address; remote joins sign in with the configured account, so pass allow_remote only with the owner's consent"
                ));
            }
            address.to_owned()
        };
        let mut reply = self.send(
            &Command::Connect {
                address: address.clone(),
            },
            CALL_TIMEOUT,
        )?;
        if let (Some(server), Some(object)) = (&self.local_server, reply.as_object_mut()) {
            object.insert("local_server_log".into(), json!(server.log));
        }
        Ok(reply)
    }

    fn start_local_server(&mut self, options: &Value) -> Result<String, String> {
        if let Some(server) = &self.local_server {
            return Ok(server.address.clone());
        }
        let path = |key: &str, default: PathBuf| {
            options
                .get(key)
                .and_then(Value::as_str)
                .map_or(default, |path| resolve(&self.repo, path))
        };
        let binary = path(
            "binary",
            executable(&self.repo, LOCAL_SERVER_BINARY, EXE_SUFFIX),
        );
        let world = path("world_dir", self.control_dir.join(SHOWCASE_WORLD));
        let extra: Vec<String> = match options.get("args") {
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| "`local_server.args` must be an array of strings")?,
            None => Vec::new(),
        };
        let log = commands::generated(&self.control_dir, "logs", "local-server.log");
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let server = LocalServer::start(&binary, &world, &extra, log)?;
        let address = server.address.clone();
        self.local_server = Some(server);
        Ok(address)
    }

    fn screenshot(&mut self, arguments: &Value) -> Value {
        let result = commands::command("screenshot", arguments, &self.repo, &self.control_dir)
            .and_then(|(command, timeout)| self.send(&command, timeout))
            .and_then(|reply| {
                let path = reply["path"].as_str().ok_or("the client sent no path")?;
                let inline = arguments.get("inline").and_then(Value::as_bool) != Some(false);
                let width = arguments
                    .get("max_width")
                    .and_then(Value::as_u64)
                    .map_or(DEFAULT_INLINE_WIDTH, |width| width as u32);
                let png = inline
                    .then(|| inline_png(Path::new(path), width))
                    .transpose()?;
                Ok((reply, png))
            });
        match result {
            Ok((reply, png)) => {
                let mut content = vec![json!({ "type": "text", "text": reply.to_string() })];
                content.extend(png.as_deref().map(png_content));
                json!({ "content": content, "isError": false })
            }
            Err(message) => text_content(&json!({ "error": message }), true),
        }
    }

    fn quit(&mut self) -> Value {
        let requested = self.send(&Command::Quit, CALL_TIMEOUT).is_ok();
        self.controller = None;
        self.endpoint = None;
        if let Some(mut client) = self.client.take() {
            client.stop();
        }
        let server_stopped = self.local_server.take().is_some();
        json!({ "quit_requested": requested, "local_server_stopped": server_stopped })
    }
}

/// `relative` under the checkout with the platform's executable `suffix`.
pub(crate) fn executable(repo: &Path, relative: &str, suffix: &str) -> PathBuf {
    repo.join(format!("{relative}{suffix}"))
}

/// The saved PNG, downscaled to at most `max_width` for inline content.
fn inline_png(path: &Path, max_width: u32) -> Result<Vec<u8>, String> {
    let image = image::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let image = if image.width() > max_width && max_width > 0 {
        let height = (u64::from(image.height()) * u64::from(max_width) / u64::from(image.width()))
            .max(1) as u32;
        image.resize_exact(max_width, height, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(png)
}
