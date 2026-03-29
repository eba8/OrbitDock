//! `orbitdock install-hooks` — configure Claude Code and Codex hooks.
//!
//! Safely merges OrbitDock hook entries into provider config files.
//! Hooks invoke `orbitdock hook-forward ...` directly; no shell script
//! install is required.

use std::io::{Read, Write};
#[cfg(unix)]
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::{fs::OpenOptions, os::fd::AsRawFd};

use crate::infrastructure::paths;

use super::hook_forward;

/// All Claude Code hook types we register for.
const CLAUDE_HOOK_TYPES: &[(&str, &str)] = &[
  ("hooks.SessionStart", "claude-session-start"),
  ("hooks.SessionEnd", "claude-session-end"),
  ("hooks.UserPromptSubmit", "claude-status-event"),
  ("hooks.Stop", "claude-status-event"),
  ("hooks.Notification", "claude-status-event"),
  ("hooks.PreCompact", "claude-status-event"),
  ("hooks.TeammateIdle", "claude-status-event"),
  ("hooks.TaskCompleted", "claude-status-event"),
  ("hooks.ConfigChange", "claude-status-event"),
  ("hooks.PreToolUse", "claude-tool-event"),
  ("hooks.PostToolUse", "claude-tool-event"),
  ("hooks.PostToolUseFailure", "claude-tool-event"),
  ("hooks.PermissionRequest", "claude-tool-event"),
  ("hooks.SubagentStart", "claude-subagent-event"),
  ("hooks.SubagentStop", "claude-subagent-event"),
  // Intentionally excluded:
  // - WorktreeCreate: adding this hook replaces Claude's default git worktree behavior.
  // - WorktreeRemove: companion cleanup hook for custom worktree providers.
];

/// All Codex hook types we register for.
const CODEX_HOOK_TYPES: &[(&str, Option<&str>, &str)] = &[
  (
    "SessionStart",
    Some("startup|resume"),
    "codex-session-start",
  ),
  ("UserPromptSubmit", None, "codex-user-prompt-submit"),
  ("Stop", None, "codex-stop-event"),
  ("PreToolUse", Some("Bash"), "codex-tool-event"),
  ("PostToolUse", Some("Bash"), "codex-tool-event"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookInstallTarget {
  Claude,
  Codex,
  Both,
}

const DEFAULT_INSTALL_TARGET: HookInstallTarget = HookInstallTarget::Both;

impl HookInstallTarget {
  fn includes_claude(self) -> bool {
    matches!(self, Self::Claude | Self::Both)
  }

  fn includes_codex(self) -> bool {
    matches!(self, Self::Codex | Self::Both)
  }
}

#[derive(Debug, Eq, PartialEq)]
struct HookInstallPlan {
  target_url: String,
  hook_binary: String,
  explicit_auth_token: Option<String>,
  auth_token_required: bool,
}

#[derive(Debug)]
struct HookMergeOutcome {
  settings: serde_json::Value,
  added: Vec<String>,
  updated: Vec<String>,
}

#[derive(Debug)]
struct CodexConfigMergeOutcome {
  document: toml::Value,
  changed: bool,
}

#[derive(Debug)]
struct ClaudeInstallOutcome {
  settings_file: PathBuf,
  merge: HookMergeOutcome,
}

#[derive(Debug)]
struct CodexInstallOutcome {
  config_path: PathBuf,
  hooks_path: PathBuf,
  config_changed: bool,
  hook_merge: HookMergeOutcome,
}

pub fn install_hooks(
  target: Option<HookInstallTarget>,
  settings_path: Option<&Path>,
  codex_config_path: Option<&Path>,
  codex_hooks_path: Option<&Path>,
  server_url: Option<&str>,
  auth_token: Option<&str>,
) -> anyhow::Result<()> {
  let installer_mode = installer_mode();
  let target = resolve_install_target(target)?;

  let install_plan = plan_hook_install(
    server_url,
    auth_token,
    &quote_for_shell(&resolve_hook_binary_path()),
  );
  let resolved_auth_token = resolve_auth_token(&install_plan)?;
  let transport_config_path =
    hook_forward::write_transport_config(&install_plan.target_url, resolved_auth_token.as_deref())?;

  let claude = if target.includes_claude() {
    Some(install_claude_settings(
      settings_path,
      &install_plan,
      installer_mode,
    )?)
  } else {
    None
  };

  let codex = if target.includes_codex() {
    Some(install_codex_settings(
      codex_config_path,
      codex_hooks_path,
      &install_plan,
      installer_mode,
    )?)
  } else {
    None
  };

  print_install_summary(
    installer_mode,
    claude.as_ref(),
    codex.as_ref(),
    &transport_config_path,
    &install_plan,
    resolved_auth_token.as_deref(),
  );

  Ok(())
}

pub fn install_claude_hooks(
  settings_path: Option<&Path>,
  server_url: Option<&str>,
  auth_token: Option<&str>,
) -> anyhow::Result<()> {
  install_hooks(
    Some(HookInstallTarget::Claude),
    settings_path,
    None,
    None,
    server_url,
    auth_token,
  )
}

fn installer_mode() -> bool {
  std::env::var_os("ORBITDOCK_INSTALLER_MODE").is_some()
}

fn resolve_install_target(target: Option<HookInstallTarget>) -> anyhow::Result<HookInstallTarget> {
  if let Some(target) = target {
    return Ok(target);
  }

  Ok(prompt_install_target()?.unwrap_or(DEFAULT_INSTALL_TARGET))
}

fn plan_hook_install(
  server_url: Option<&str>,
  auth_token: Option<&str>,
  hook_binary: &str,
) -> HookInstallPlan {
  let target_url = server_url
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(crate::admin::normalize_client_server_url)
    .unwrap_or_else(|| "http://127.0.0.1:4000".to_string());
  let explicit_auth_token = normalized_non_empty(auth_token);
  let auth_token_required =
    explicit_auth_token.is_none() && should_prompt_for_auth_token(&target_url);

  HookInstallPlan {
    target_url,
    hook_binary: hook_binary.to_string(),
    explicit_auth_token,
    auth_token_required,
  }
}

fn resolve_auth_token(plan: &HookInstallPlan) -> anyhow::Result<Option<String>> {
  if let Some(token) = plan.explicit_auth_token.as_ref() {
    return Ok(Some(token.clone()));
  }

  // Preserve the existing token from hook-forward.json (e.g., provisioned by `init`)
  if let Some(existing) = hook_forward::read_transport_config()
    .ok()
    .flatten()
    .and_then(|cfg| cfg.auth_token())
  {
    return Ok(Some(existing));
  }

  if !plan.auth_token_required {
    return Ok(None);
  }

  prompt_auth_token(&plan.target_url)
}

fn should_prompt_for_auth_token(server_url: &str) -> bool {
  !is_local_server_url(server_url)
}

fn is_local_server_url(server_url: &str) -> bool {
  let trimmed = server_url.trim();
  if trimmed.is_empty() {
    return true;
  }

  if let Ok(url) = reqwest::Url::parse(trimmed) {
    return url.host_str().map(is_local_host).unwrap_or(false);
  }

  trimmed.contains("127.0.0.1")
    || trimmed.contains("localhost")
    || trimmed.contains("[::1]")
    || trimmed.contains("://0.0.0.0")
}

fn is_local_host(host: &str) -> bool {
  matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0")
}

fn normalized_non_empty(value: Option<&str>) -> Option<String> {
  let value = value?;
  let trimmed = value.trim();
  if trimmed.is_empty() {
    None
  } else {
    Some(trimmed.to_string())
  }
}

fn install_claude_settings(
  settings_path: Option<&Path>,
  plan: &HookInstallPlan,
  installer_mode: bool,
) -> anyhow::Result<ClaudeInstallOutcome> {
  let settings_file = settings_path.map(PathBuf::from).unwrap_or_else(|| {
    dirs::home_dir()
      .expect("HOME not found")
      .join(".claude/settings.json")
  });

  let existing = if settings_file.exists() {
    let content = std::fs::read_to_string(&settings_file)?;
    serde_json::from_str::<serde_json::Value>(&content)?
  } else {
    serde_json::json!({})
  };

  let merge = merge_claude_hooks(existing, plan)?;

  if settings_file.exists() {
    backup_file(&settings_file, installer_mode)?;
  }

  if let Some(parent) = settings_file.parent() {
    std::fs::create_dir_all(parent)?;
  }

  let formatted = serde_json::to_string_pretty(&merge.settings)?;
  std::fs::write(&settings_file, formatted)?;

  Ok(ClaudeInstallOutcome {
    settings_file,
    merge,
  })
}

fn install_codex_settings(
  codex_config_path: Option<&Path>,
  codex_hooks_path: Option<&Path>,
  plan: &HookInstallPlan,
  installer_mode: bool,
) -> anyhow::Result<CodexInstallOutcome> {
  let codex_root = dirs::home_dir().expect("HOME not found").join(".codex");
  let config_path = codex_config_path
    .map(PathBuf::from)
    .unwrap_or_else(|| codex_root.join("config.toml"));
  let hooks_path = codex_hooks_path
    .map(PathBuf::from)
    .unwrap_or_else(|| codex_root.join("hooks.json"));

  let existing_config = if config_path.exists() {
    let content = std::fs::read_to_string(&config_path)?;
    toml::from_str::<toml::Value>(&content)?
  } else {
    toml::Value::Table(Default::default())
  };
  let config_merge = merge_codex_config(existing_config)?;

  let existing_hooks = if hooks_path.exists() {
    let content = std::fs::read_to_string(&hooks_path)?;
    serde_json::from_str::<serde_json::Value>(&content)?
  } else {
    serde_json::json!({})
  };
  let hook_merge = merge_codex_hooks(existing_hooks, plan)?;

  if config_path.exists() {
    backup_file(&config_path, installer_mode)?;
  }
  if hooks_path.exists() {
    backup_file(&hooks_path, installer_mode)?;
  }

  if let Some(parent) = config_path.parent() {
    std::fs::create_dir_all(parent)?;
  }
  if let Some(parent) = hooks_path.parent() {
    std::fs::create_dir_all(parent)?;
  }

  std::fs::write(
    &config_path,
    toml::to_string_pretty(&config_merge.document)?,
  )?;
  std::fs::write(
    &hooks_path,
    serde_json::to_string_pretty(&hook_merge.settings)?,
  )?;

  Ok(CodexInstallOutcome {
    config_path,
    hooks_path,
    config_changed: config_merge.changed,
    hook_merge,
  })
}

fn backup_file(path: &Path, installer_mode: bool) -> anyhow::Result<()> {
  let backup = match path.extension().and_then(|ext| ext.to_str()) {
    Some(ext) if !ext.is_empty() => path.with_extension(format!("{ext}.bak")),
    _ => path.with_extension("bak"),
  };
  std::fs::copy(path, &backup)?;
  if !installer_mode {
    println!("  Backed up {} → {}", path.display(), backup.display());
  }
  Ok(())
}

fn merge_claude_hooks(
  existing: serde_json::Value,
  plan: &HookInstallPlan,
) -> anyhow::Result<HookMergeOutcome> {
  let mut settings = existing;
  let obj = settings
    .as_object_mut()
    .ok_or_else(|| anyhow::anyhow!("settings.json is not a JSON object"))?;

  let mut added = Vec::new();
  let mut updated = Vec::new();

  for &(hook_key, hook_type) in CLAUDE_HOOK_TYPES {
    let command = format!("{} hook-forward {hook_type}", plan.hook_binary);

    let Some((parent_key, child_key)) = hook_key.split_once('.') else {
      return Err(anyhow::anyhow!("invalid hook key '{hook_key}'"));
    };

    let hooks_obj = obj
      .entry(parent_key)
      .or_insert_with(|| serde_json::json!({}));
    let hooks_map = hooks_obj
      .as_object_mut()
      .ok_or_else(|| anyhow::anyhow!("settings.json '{}' is not an object", parent_key))?;

    let hook_entry = orbitdock_hook_entry(&command);

    if let Some(existing_hooks) = hooks_map.get_mut(child_key) {
      if !existing_hooks.is_array() {
        let normalized = existing_hooks.take();
        *existing_hooks = serde_json::json!([normalized]);
      }
      if let Some(arr) = existing_hooks.as_array_mut() {
        if let Some(idx) = orbitdock_hook_index(arr) {
          arr[idx] = hook_entry;
          updated.push(hook_key.to_string());
        } else {
          arr.push(hook_entry);
          added.push(hook_key.to_string());
        }
      }
    } else {
      hooks_map.insert(child_key.to_string(), serde_json::json!([hook_entry]));
      added.push(hook_key.to_string());
    }
  }

  Ok(HookMergeOutcome {
    settings,
    added,
    updated,
  })
}

fn merge_codex_hooks(
  existing: serde_json::Value,
  plan: &HookInstallPlan,
) -> anyhow::Result<HookMergeOutcome> {
  let mut settings = existing;
  let obj = settings
    .as_object_mut()
    .ok_or_else(|| anyhow::anyhow!("hooks.json is not a JSON object"))?;
  let hooks_obj = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
  let hooks_map = hooks_obj
    .as_object_mut()
    .ok_or_else(|| anyhow::anyhow!("hooks.json 'hooks' is not an object"))?;

  let mut added = Vec::new();
  let mut updated = Vec::new();

  for &(event_name, matcher, hook_type) in CODEX_HOOK_TYPES {
    let command = format!("{} hook-forward {hook_type}", plan.hook_binary);
    let hook_entry = orbitdock_codex_hook_entry(&command, matcher);

    if let Some(existing_hooks) = hooks_map.get_mut(event_name) {
      if !existing_hooks.is_array() {
        let normalized = existing_hooks.take();
        *existing_hooks = serde_json::json!([normalized]);
      }
      if let Some(arr) = existing_hooks.as_array_mut() {
        if let Some(idx) = orbitdock_hook_index(arr) {
          arr[idx] = hook_entry;
          updated.push(format!("hooks.{event_name}"));
        } else {
          arr.push(hook_entry);
          added.push(format!("hooks.{event_name}"));
        }
      }
    } else {
      hooks_map.insert(event_name.to_string(), serde_json::json!([hook_entry]));
      added.push(format!("hooks.{event_name}"));
    }
  }

  Ok(HookMergeOutcome {
    settings,
    added,
    updated,
  })
}

fn merge_codex_config(existing: toml::Value) -> anyhow::Result<CodexConfigMergeOutcome> {
  let mut document = existing;
  let root = document
    .as_table_mut()
    .ok_or_else(|| anyhow::anyhow!("config.toml is not a TOML table"))?;

  let features = root
    .entry("features")
    .or_insert_with(|| toml::Value::Table(Default::default()));
  let features_table = features
    .as_table_mut()
    .ok_or_else(|| anyhow::anyhow!("config.toml [features] is not a TOML table"))?;

  let changed = features_table
    .get("codex_hooks")
    .and_then(toml::Value::as_bool)
    != Some(true);
  features_table.insert("codex_hooks".to_string(), toml::Value::Boolean(true));

  Ok(CodexConfigMergeOutcome { document, changed })
}

fn orbitdock_hook_entry(command: &str) -> serde_json::Value {
  serde_json::json!({
      "hooks": [{
          "type": "command",
          "command": command,
          "async": true
      }]
  })
}

fn orbitdock_codex_hook_entry(command: &str, matcher: Option<&str>) -> serde_json::Value {
  let mut entry = serde_json::json!({
    "hooks": [{
      "type": "command",
      "command": command
    }]
  });
  if let Some(matcher) = matcher {
    entry["matcher"] = serde_json::Value::String(matcher.to_string());
  }
  entry
}

fn orbitdock_hook_index(entries: &[serde_json::Value]) -> Option<usize> {
  entries.iter().position(entry_contains_orbitdock_command)
}

fn entry_contains_orbitdock_command(entry: &serde_json::Value) -> bool {
  if let Some(hooks_arr) = entry.get("hooks").and_then(|hooks| hooks.as_array()) {
    return hooks_arr.iter().any(command_value_is_orbitdock);
  }

  command_value_is_orbitdock(entry)
}

fn command_value_is_orbitdock(value: &serde_json::Value) -> bool {
  value
    .get("command")
    .and_then(|command| command.as_str())
    .map(|command| {
      command.contains("orbitdock")
        || command.contains("hook.sh")
        || command.contains("hook-forward")
    })
    .unwrap_or(false)
}

fn print_install_summary(
  installer_mode: bool,
  claude: Option<&ClaudeInstallOutcome>,
  codex: Option<&CodexInstallOutcome>,
  transport_config_path: &Path,
  plan: &HookInstallPlan,
  resolved_auth_token: Option<&str>,
) {
  println!();
  if installer_mode {
    if let Some(claude) = claude {
      println!(
        "  Claude Code hooks ready in {}",
        claude.settings_file.display()
      );
    }
    if let Some(codex) = codex {
      println!("  Codex hooks ready in {}", codex.hooks_path.display());
    }
  } else {
    if let Some(claude) = claude {
      print_provider_merge_summary("Claude", &claude.merge);
      println!("  Claude settings: {}", claude.settings_file.display());
    }
    if let Some(codex) = codex {
      print_provider_merge_summary("Codex", &codex.hook_merge);
      println!("  Codex config: {}", codex.config_path.display());
      println!("  Codex hooks: {}", codex.hooks_path.display());
      println!(
        "  Codex feature flag: {}",
        if codex.config_changed {
          "enabled codex_hooks"
        } else {
          "already enabled"
        }
      );
    }
    if claude.is_some() || codex.is_some() {
      println!();
    }
  }
  println!(
    "  Hook transport config: {}",
    transport_config_path.display()
  );
  match resolved_auth_token {
    Some(_) => println!("  Hook auth token: configured"),
    None if plan.auth_token_required => {
      println!("  Hook auth token: not configured");
      println!(
        "  Remote requests may be rejected until you rerun `orbitdock install-hooks` with a token."
      );
    }
    None if !installer_mode => println!("  Hook auth token: not configured"),
    None => {}
  }
  if !installer_mode {
    println!("  Hook forward binary: {}", resolve_hook_binary_path());
    println!("  Spool directory: {}", paths::spool_dir().display());
  }
  println!();
}

fn print_provider_merge_summary(provider: &str, merge: &HookMergeOutcome) {
  if !merge.added.is_empty() {
    println!("  Added {} {provider} hook(s):", merge.added.len());
    for hook in &merge.added {
      println!("    + {}", hook);
    }
  }
  if !merge.updated.is_empty() {
    println!("  Updated {} {provider} hook(s):", merge.updated.len());
    for hook in &merge.updated {
      println!("    ~ {}", hook);
    }
  }
}

#[cfg(unix)]
fn prompt_install_target() -> anyhow::Result<Option<HookInstallTarget>> {
  let mut tty = match OpenOptions::new().read(true).write(true).open("/dev/tty") {
    Ok(file) => file,
    Err(_) => return Ok(None),
  };

  writeln!(tty)?;
  writeln!(tty, "  Install hooks for which provider?")?;
  writeln!(tty, "    1. Claude")?;
  writeln!(tty, "    2. Codex")?;
  writeln!(tty, "    3. Both")?;

  loop {
    write!(tty, "  Selection [3]: ")?;
    tty.flush()?;
    let response = read_visible_line(&mut tty)?;
    let choice = response.trim().to_ascii_lowercase();
    match choice.as_str() {
      "" | "3" | "both" | "b" => return Ok(Some(HookInstallTarget::Both)),
      "1" | "claude" | "c" => return Ok(Some(HookInstallTarget::Claude)),
      "2" | "codex" => return Ok(Some(HookInstallTarget::Codex)),
      _ => {
        writeln!(tty, "  Please choose 1, 2, or 3.")?;
      }
    }
  }
}

#[cfg(unix)]
fn prompt_auth_token(server_url: &str) -> anyhow::Result<Option<String>> {
  let mut tty = match OpenOptions::new().read(true).write(true).open("/dev/tty") {
    Ok(file) => file,
    Err(_) => return Ok(None),
  };

  writeln!(tty)?;
  writeln!(tty, "  Remote server detected: {server_url}")?;
  writeln!(
    tty,
    "  Enter the auth token to store in the local hook transport config."
  )?;
  writeln!(
    tty,
    "  Press Enter only if this remote server is intentionally running without auth."
  )?;
  write!(tty, "  Auth token: ")?;
  tty.flush()?;

  let token = read_hidden_line(&mut tty)?;
  writeln!(tty)?;
  Ok(normalized_non_empty(Some(token.as_str())))
}

#[cfg(not(unix))]
fn prompt_install_target() -> anyhow::Result<Option<HookInstallTarget>> {
  Ok(None)
}

#[cfg(not(unix))]
fn prompt_auth_token(_server_url: &str) -> anyhow::Result<Option<String>> {
  Ok(None)
}

#[cfg(unix)]
fn read_visible_line(file: &mut std::fs::File) -> anyhow::Result<String> {
  let mut line = String::new();
  loop {
    let mut buf = [0u8; 1];
    match file.read(&mut buf) {
      Ok(0) => break,
      Ok(_) if matches!(buf[0], b'\n' | b'\r') => break,
      Ok(_) => line.push(buf[0] as char),
      Err(err) => return Err(err.into()),
    }
  }
  Ok(line)
}

#[cfg(unix)]
fn read_hidden_line(file: &mut std::fs::File) -> anyhow::Result<String> {
  let fd = file.as_raw_fd();
  let original = current_termios(fd)?;
  let mut hidden = original;
  hidden.c_lflag &= !libc::ECHO;

  if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &hidden) } != 0 {
    return Err(std::io::Error::last_os_error().into());
  }

  let _restore = TermiosRestoreGuard { fd, original };
  let mut bytes = Vec::new();

  loop {
    let mut buf = [0u8; 1];
    match file.read(&mut buf) {
      Ok(0) => break,
      Ok(_) if matches!(buf[0], b'\n' | b'\r') => break,
      Ok(_) => bytes.push(buf[0]),
      Err(err) => return Err(err.into()),
    }
  }

  Ok(String::from_utf8(bytes)?)
}

#[cfg(unix)]
fn current_termios(fd: i32) -> anyhow::Result<libc::termios> {
  let mut termios = MaybeUninit::<libc::termios>::uninit();
  if unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) } != 0 {
    return Err(std::io::Error::last_os_error().into());
  }
  Ok(unsafe { termios.assume_init() })
}

#[cfg(unix)]
struct TermiosRestoreGuard {
  fd: i32,
  original: libc::termios,
}

#[cfg(unix)]
impl Drop for TermiosRestoreGuard {
  fn drop(&mut self) {
    let _ = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.original) };
  }
}

fn resolve_hook_binary_path() -> String {
  std::env::current_exe()
    .ok()
    .and_then(|path| path.into_os_string().into_string().ok())
    .unwrap_or_else(|| "orbitdock".to_string())
}

fn quote_for_shell(path: &str) -> String {
  format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
  use super::{
    command_value_is_orbitdock, entry_contains_orbitdock_command, merge_claude_hooks,
    merge_codex_config, merge_codex_hooks, orbitdock_hook_index, plan_hook_install,
    HookInstallPlan,
  };

  fn test_plan() -> HookInstallPlan {
    plan_hook_install(
      Some("http://127.0.0.1:4000"),
      Some("token-123"),
      "\"/usr/local/bin/orbitdock\"",
    )
  }

  #[test]
  fn hook_install_plan_only_requires_token_for_remote_servers() {
    let local = plan_hook_install(
      Some("http://127.0.0.1:4000"),
      None,
      "\"/usr/local/bin/orbitdock\"",
    );
    let remote = plan_hook_install(
      Some("https://orbitdock.example.com"),
      None,
      "\"/usr/local/bin/orbitdock\"",
    );
    let remote_with_explicit_token = plan_hook_install(
      Some("https://orbitdock.example.com"),
      Some("secret"),
      "\"/usr/local/bin/orbitdock\"",
    );

    assert!(!local.auth_token_required);
    assert!(remote.auth_token_required);
    assert!(!remote_with_explicit_token.auth_token_required);
    assert_eq!(
      remote_with_explicit_token.explicit_auth_token.as_deref(),
      Some("secret")
    );
  }

  #[test]
  fn merge_claude_hooks_adds_missing_hook_entries() {
    let merge = merge_claude_hooks(serde_json::json!({}), &test_plan()).unwrap();
    let hooks = merge
      .settings
      .get("hooks")
      .and_then(|value| value.as_object())
      .unwrap();

    assert_eq!(merge.added.len(), super::CLAUDE_HOOK_TYPES.len());
    assert!(merge.updated.is_empty());
    assert!(hooks.contains_key("SessionStart"));
    assert!(hooks.contains_key("PermissionRequest"));
  }

  #[test]
  fn merge_claude_hooks_replaces_legacy_entry_without_duplication() {
    let existing = serde_json::json!({
        "hooks": {
            "Notification": {
                "command": "hook.sh claude_status_event"
            }
        }
    });

    let merge = merge_claude_hooks(existing, &test_plan()).unwrap();
    let notification = merge
      .settings
      .get("hooks")
      .and_then(|value| value.get("Notification"))
      .and_then(|value| value.as_array())
      .unwrap();

    assert!(merge
      .updated
      .iter()
      .any(|hook| hook == "hooks.Notification"));
    assert_eq!(notification.len(), 1);
    assert!(entry_contains_orbitdock_command(&notification[0]));
  }

  #[test]
  fn merge_claude_hooks_preserves_non_orbitdock_entries_when_adding_new_one() {
    let existing = serde_json::json!({
        "hooks": {
            "Notification": [{
                "hooks": [{
                    "type": "command",
                    "command": "python notify.py",
                    "async": true
                }]
            }]
        }
    });

    let merge = merge_claude_hooks(existing, &test_plan()).unwrap();
    let notification = merge
      .settings
      .get("hooks")
      .and_then(|value| value.get("Notification"))
      .and_then(|value| value.as_array())
      .unwrap();

    assert!(merge.added.iter().any(|hook| hook == "hooks.Notification"));
    assert_eq!(notification.len(), 2);
    assert!(!entry_contains_orbitdock_command(&notification[0]));
    assert!(entry_contains_orbitdock_command(&notification[1]));
  }

  #[test]
  fn orbitdock_command_detection_handles_nested_and_bare_entries() {
    let nested = serde_json::json!({
        "hooks": [{
            "command": "\"/usr/local/bin/orbitdock\" hook-forward claude_status_event"
        }]
    });
    let bare = serde_json::json!({
        "command": "hook.sh claude_status_event"
    });
    let unrelated = serde_json::json!({
        "command": "python notify.py"
    });

    assert!(entry_contains_orbitdock_command(&nested));
    assert!(entry_contains_orbitdock_command(&bare));
    assert!(command_value_is_orbitdock(&bare));
    assert!(!entry_contains_orbitdock_command(&unrelated));
    assert_eq!(orbitdock_hook_index(&[unrelated.clone(), nested]), Some(1));
  }

  #[test]
  fn merge_codex_hooks_adds_missing_hook_entries() {
    let merge = merge_codex_hooks(serde_json::json!({}), &test_plan()).unwrap();
    let hooks = merge
      .settings
      .get("hooks")
      .and_then(|value| value.as_object())
      .unwrap();

    assert_eq!(merge.added.len(), super::CODEX_HOOK_TYPES.len());
    assert!(hooks.contains_key("SessionStart"));
    assert!(hooks.contains_key("PostToolUse"));
  }

  #[test]
  fn merge_codex_hooks_preserves_existing_non_orbitdock_entries() {
    let existing = serde_json::json!({
      "hooks": {
        "PreToolUse": [{
          "matcher": "Bash",
          "hooks": [{
            "type": "command",
            "command": "python pre_tool.py"
          }]
        }]
      }
    });

    let merge = merge_codex_hooks(existing, &test_plan()).unwrap();
    let entries = merge
      .settings
      .get("hooks")
      .and_then(|value| value.get("PreToolUse"))
      .and_then(|value| value.as_array())
      .unwrap();

    assert_eq!(entries.len(), 2);
    assert!(!entry_contains_orbitdock_command(&entries[0]));
    assert!(entry_contains_orbitdock_command(&entries[1]));
  }

  #[test]
  fn merge_codex_config_enables_feature_flag() {
    let merge = merge_codex_config(toml::Value::Table(toml::toml! {
      [sandbox_workspace_write]
      network_access = false
    }))
    .unwrap();

    assert!(merge.changed);
    let features = merge
      .document
      .get("features")
      .and_then(toml::Value::as_table)
      .unwrap();
    assert_eq!(
      features.get("codex_hooks").and_then(toml::Value::as_bool),
      Some(true)
    );
  }

  #[test]
  fn default_install_target_matches_prompt_default() {
    assert_eq!(
      super::DEFAULT_INSTALL_TARGET,
      super::HookInstallTarget::Both
    );
  }
}
