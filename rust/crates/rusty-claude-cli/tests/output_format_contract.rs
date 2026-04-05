use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use mock_anthropic_service::{MockAnthropicService, SCENARIO_PREFIX};
use serde_json::{json, Value};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn help_emits_json_when_requested() {
    let root = unique_temp_dir("help-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "help"], &envs);

    assert_eq!(parsed["kind"], "help");
    assert!(parsed["message"]
        .as_str()
        .expect("help message")
        .contains("Usage:"));
}

#[test]
fn version_emits_json_when_requested() {
    let root = unique_temp_dir("version-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "version"], &envs);

    assert_eq!(parsed["kind"], "version");
    assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn status_emits_json_when_requested() {
    let root = unique_temp_dir("status-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "status"], &envs);

    assert_eq!(parsed["kind"], "status");
    assert!(parsed["workspace"]["cwd"].as_str().is_some());
}

#[test]
fn sandbox_emits_json_when_requested() {
    let root = unique_temp_dir("sandbox-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "sandbox"], &envs);

    assert_eq!(parsed["kind"], "sandbox");
    assert!(parsed["sandbox"].is_object());
}

#[test]
fn dump_manifests_emits_json_when_requested() {
    let root = unique_temp_dir("dump-manifests-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let upstream = write_upstream_fixture(&root);
    let mut envs = isolated_env(&root);
    envs.push((
        "CLAUDE_CODE_UPSTREAM".to_string(),
        upstream.display().to_string(),
    ));

    let parsed = assert_json_command(&root, &["--output-format", "json", "dump-manifests"], &envs);

    assert_eq!(parsed["kind"], "dump-manifests");
    assert_eq!(parsed["commands"], 1);
    assert_eq!(parsed["tools"], 1);
}

#[test]
fn bootstrap_plan_emits_json_when_requested() {
    let root = unique_temp_dir("bootstrap-plan-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "bootstrap-plan"], &envs);

    assert_eq!(parsed["kind"], "bootstrap-plan");
    assert!(parsed["phases"].as_array().expect("phases array").len() > 1);
}

#[test]
fn agents_emits_json_when_requested() {
    let root = unique_temp_dir("agents-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "agents"], &envs);

    assert_eq!(parsed["kind"], "agents");
    assert!(!parsed["message"].as_str().expect("agents text").is_empty());
}

#[test]
fn mcp_emits_json_when_requested() {
    let root = unique_temp_dir("mcp-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "mcp"], &envs);

    assert_eq!(parsed["kind"], "mcp");
    assert!(parsed["message"]
        .as_str()
        .expect("mcp text")
        .contains("MCP"));
}

#[test]
fn skills_emits_json_when_requested() {
    let root = unique_temp_dir("skills-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "skills"], &envs);

    assert_eq!(parsed["kind"], "skills");
    assert!(!parsed["message"].as_str().expect("skills text").is_empty());
}

#[test]
fn system_prompt_emits_json_when_requested() {
    let root = unique_temp_dir("system-prompt-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "system-prompt"], &envs);

    assert_eq!(parsed["kind"], "system-prompt");
    assert!(parsed["message"]
        .as_str()
        .expect("system prompt text")
        .contains("You are an interactive agent"));
}

#[test]
fn login_emits_json_when_requested() {
    let root = unique_temp_dir("login-json");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).expect("workspace should exist");
    let mut envs = isolated_env(&root);
    let callback_port = reserve_port();
    let token_port = reserve_port();

    fs::create_dir_all(workspace.join(".claw")).expect("config dir should exist");
    fs::write(
        workspace.join(".claw").join("settings.json"),
        json!({
            "oauth": {
                "clientId": "test-client",
                "authorizeUrl": format!("http://127.0.0.1:{token_port}/authorize"),
                "tokenUrl": format!("http://127.0.0.1:{token_port}/token"),
                "callbackPort": callback_port,
                "scopes": ["user:test"]
            }
        })
        .to_string(),
    )
    .expect("oauth config should write");

    let token_server = thread::spawn(move || {
        let listener = TcpListener::bind(("127.0.0.1", token_port)).expect("token server bind");
        let (mut stream, _) = listener.accept().expect("token request");
        let mut request = [0_u8; 4096];
        let _ = stream
            .read(&mut request)
            .expect("token request should read");
        let body = json!({
            "access_token": "test-access-token",
            "refresh_token": "test-refresh-token",
            "expires_at": 9_999_999_999_u64,
            "scopes": ["user:test"]
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("token response should write");
    });

    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir should exist");
    let opener_path = bin_dir.join("xdg-open");
    fs::write(
        &opener_path,
        format!(
            "#!/usr/bin/env python3\nimport http.client\nimport sys\nimport urllib.parse\nurl = sys.argv[1]\nquery = urllib.parse.parse_qs(urllib.parse.urlparse(url).query)\nstate = query['state'][0]\nconn = http.client.HTTPConnection('127.0.0.1', {callback_port}, timeout=5)\nconn.request('GET', f\"/callback?code=test-code&state={{urllib.parse.quote(state)}}\")\nresp = conn.getresponse()\nresp.read()\nconn.close()\n"
        ),
    )
    .expect("xdg-open wrapper should write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&opener_path)
            .expect("wrapper metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&opener_path, permissions).expect("wrapper permissions");
    }
    let original_path = envs
        .iter()
        .find(|(key, _)| key == "PATH")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    for (key, value) in &mut envs {
        if key == "PATH" {
            *value = format!("{}:{original_path}", bin_dir.display());
        }
    }

    let parsed = assert_json_command(&workspace, &["--output-format", "json", "login"], &envs);

    token_server.join().expect("token server should finish");
    assert_eq!(parsed["kind"], "login");
    assert_eq!(parsed["callback_port"], callback_port);
}

#[test]
fn logout_emits_json_when_requested() {
    let root = unique_temp_dir("logout-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&root, &["--output-format", "json", "logout"], &envs);

    assert_eq!(parsed["kind"], "logout");
    assert!(parsed["message"]
        .as_str()
        .expect("logout text")
        .contains("cleared"));
}

#[test]
fn init_emits_json_when_requested() {
    let root = unique_temp_dir("init-json");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).expect("workspace should exist");
    let envs = isolated_env(&root);

    let parsed = assert_json_command(&workspace, &["--output-format", "json", "init"], &envs);

    assert_eq!(parsed["kind"], "init");
    assert!(workspace.join("CLAUDE.md").exists());
}

#[test]
fn prompt_subcommand_emits_json_when_requested() {
    let root = unique_temp_dir("prompt-subcommand-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let mut envs = isolated_env(&root);
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should build");
    let server = runtime
        .block_on(MockAnthropicService::spawn())
        .expect("mock service should start");
    envs.push(("ANTHROPIC_API_KEY".to_string(), "test-key".to_string()));
    envs.push(("ANTHROPIC_BASE_URL".to_string(), server.base_url()));

    let prompt = format!("{SCENARIO_PREFIX}streaming_text");
    let args = vec![
        "--model".to_string(),
        "sonnet".to_string(),
        "--permission-mode".to_string(),
        "read-only".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "prompt".to_string(),
        prompt,
    ];
    let output = run_claw_with_env_owned(&root, &args, &envs);
    let parsed = parse_json_stdout(&output);

    assert_eq!(parsed["model"], "claude-sonnet-4-6");
    assert!(parsed["message"]
        .as_str()
        .expect("assistant text")
        .contains("streaming"));
}

#[test]
fn bare_prompt_mode_emits_json_when_requested() {
    let root = unique_temp_dir("bare-prompt-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let mut envs = isolated_env(&root);
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should build");
    let server = runtime
        .block_on(MockAnthropicService::spawn())
        .expect("mock service should start");
    envs.push(("ANTHROPIC_API_KEY".to_string(), "test-key".to_string()));
    envs.push(("ANTHROPIC_BASE_URL".to_string(), server.base_url()));

    let prompt = format!("{SCENARIO_PREFIX}streaming_text");
    let args = vec![
        "--model".to_string(),
        "sonnet".to_string(),
        "--permission-mode".to_string(),
        "read-only".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        prompt,
    ];
    let output = run_claw_with_env_owned(&root, &args, &envs);
    let parsed = parse_json_stdout(&output);

    assert_eq!(parsed["model"], "claude-sonnet-4-6");
    assert!(parsed["message"]
        .as_str()
        .expect("assistant text")
        .contains("streaming"));
}

#[test]
fn resume_restore_emits_json_when_requested() {
    let root = unique_temp_dir("resume-json");
    fs::create_dir_all(&root).expect("temp dir should exist");
    let envs = isolated_env(&root);
    let session_path = root.join("session.jsonl");
    fs::write(
        &session_path,
        "{\"type\":\"session_meta\",\"version\":3,\"session_id\":\"resume-json\",\"created_at_ms\":0,\"updated_at_ms\":0}\n{\"type\":\"message\",\"message\":{\"role\":\"user\",\"blocks\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n",
    )
    .expect("session should write");

    let args = vec![
        "--output-format".to_string(),
        "json".to_string(),
        "--resume".to_string(),
        session_path.display().to_string(),
    ];
    let output = run_claw_with_env_owned(&root, &args, &envs);
    let parsed = parse_json_stdout(&output);

    assert_eq!(parsed["kind"], "resume");
    assert_eq!(parsed["messages"], 1);
}

fn assert_json_command(current_dir: &Path, args: &[&str], envs: &[(String, String)]) -> Value {
    let output = run_claw_with_env(current_dir, args, envs);
    parse_json_stdout(&output)
}

fn parse_json_stdout(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout should be json")
}

fn run_claw_with_env(current_dir: &Path, args: &[&str], envs: &[(String, String)]) -> Output {
    let owned_args = args
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    run_claw_with_env_owned(current_dir, &owned_args, envs)
}

fn run_claw_with_env_owned(
    current_dir: &Path,
    args: &[String],
    envs: &[(String, String)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_claw"));
    command.current_dir(current_dir).args(args).env_clear();
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("claw should launch")
}

fn isolated_env(root: &Path) -> Vec<(String, String)> {
    let config_home = root.join("config-home");
    let home = root.join("home");
    fs::create_dir_all(&config_home).expect("config home should exist");
    fs::create_dir_all(&home).expect("home should exist");
    vec![
        (
            "CLAW_CONFIG_HOME".to_string(),
            config_home.display().to_string(),
        ),
        ("HOME".to_string(), home.display().to_string()),
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string()),
        ),
        ("NO_COLOR".to_string(), "1".to_string()),
    ]
}

fn write_upstream_fixture(root: &Path) -> PathBuf {
    let upstream = root.join("claw-code");
    let src = upstream.join("src");
    let entrypoints = src.join("entrypoints");
    fs::create_dir_all(&entrypoints).expect("upstream entrypoints dir should exist");
    fs::write(
        src.join("commands.ts"),
        "import FooCommand from './commands/foo'\n",
    )
    .expect("commands fixture should write");
    fs::write(
        src.join("tools.ts"),
        "import ReadTool from './tools/read'\n",
    )
    .expect("tools fixture should write");
    fs::write(
        entrypoints.join("cli.tsx"),
        "if (args[0] === '--version') {}\nstartupProfiler()\n",
    )
    .expect("cli fixture should write");
    upstream
}

fn reserve_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral port should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_millis();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "claw-output-format-{label}-{}-{millis}-{counter}",
        std::process::id()
    ))
}
