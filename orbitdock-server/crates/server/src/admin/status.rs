//! `orbitdock status` — unified server status and diagnostics.
//! `orbitdock auth` — token management helpers.

use std::path::Path;

use crate::infrastructure::auth_tokens;

/// Unified status command — delegates to the doctor diagnostics checklist.
pub fn print_server_status(data_dir: &Path) -> anyhow::Result<()> {
  super::doctor::print_diagnostics(data_dir)
}

/// Create a new auth token and store its hash in the database. Returns the token string.
pub fn issue_auth_token(data_dir: &Path) -> anyhow::Result<String> {
  let _ = data_dir;
  let issued = auth_tokens::issue_token(Some("setup"))?;
  Ok(issued.token)
}

pub fn print_generated_auth_token(data_dir: &Path) -> anyhow::Result<()> {
  let _ = data_dir;
  let issued = auth_tokens::issue_token(None)?;

  println!();
  println!("  Secure auth token generated and stored (hashed) in the database.");
  println!("  Copy it now and store it somewhere secure.");
  println!();
  println!("  Token ID: {}", issued.id);
  println!("  Token: {}", issued.token);
  println!();
  println!("  Usage:");
  println!("    orbitdock start --bind 0.0.0.0:4000");
  println!("    orbitdock install-hooks --server-url <url> --auth-token <token>");
  println!();
  println!("  Manage tokens:");
  println!("    orbitdock auth list       — see all tokens");
  println!("    orbitdock auth revoke ID  — revoke a token");
  println!("    orbitdock auth status     — check auth health");
  println!();

  Ok(())
}

pub fn print_auth_tokens() -> anyhow::Result<()> {
  let tokens = auth_tokens::list_tokens()?;

  println!();
  println!("  Auth Tokens");
  println!("  ───────────");
  println!();

  if tokens.is_empty() {
    println!("  No tokens found.");
    println!();
    return Ok(());
  }

  for token in tokens {
    let status = if token.revoked_at.is_some() {
      "revoked"
    } else {
      "active"
    };
    let label = token.label.as_deref().unwrap_or("(no label)");
    println!("  {}  [{}]  {}", token.id, status, label);
    println!("    created: {}", token.created_at);
    if let Some(ref used) = token.last_used_at {
      println!("    last used: {}", used);
    }
    if let Some(ref expires) = token.expires_at {
      println!("    expires: {}", expires);
    }
    if let Some(ref revoked) = token.revoked_at {
      println!("    revoked: {}", revoked);
    }
    println!();
  }

  Ok(())
}

/// Print the decrypted local auth token from `hook-forward.json` to stdout.
///
/// Outputs just the token (no decoration) for programmatic consumption.
/// Exits non-zero if no token is configured.
pub fn print_local_token() -> anyhow::Result<()> {
  let config = super::hook_forward::read_transport_config()?;
  let token = config
    .and_then(|cfg| cfg.auth_token())
    .ok_or_else(|| anyhow::anyhow!("no local auth token configured — run `orbitdock init`"))?;
  println!("{token}");
  Ok(())
}

/// Print auth diagnostic information.
pub fn print_auth_status() -> anyhow::Result<()> {
  println!();
  println!("  Auth Status");
  println!("  ───────────");
  println!();

  // Check hook-forward.json token
  let local_token = super::hook_forward::read_transport_config()
    .ok()
    .flatten()
    .and_then(|cfg| cfg.auth_token());

  match &local_token {
    Some(token) if token.starts_with("odtk_") => {
      println!(
        "  Local token:  configured ({}...)",
        &token[..16.min(token.len())]
      );
    }
    Some(token) => {
      println!(
        "  Local token:  INVALID — does not start with odtk_ (found: {}...)",
        &token[..8.min(token.len())]
      );
      println!("                Run `orbitdock auth reset` to fix.");
    }
    None => {
      println!("  Local token:  not configured");
      println!("                Run `orbitdock init` or `orbitdock auth reset` to fix.");
    }
  }

  // Check database tokens
  match auth_tokens::active_token_count() {
    Ok(count) if count > 0 => {
      println!("  DB tokens:    {} active", count);
    }
    Ok(_) => {
      println!("  DB tokens:    none — server will allow unauthenticated access");
    }
    Err(e) => {
      println!("  DB tokens:    error reading database ({})", e);
    }
  }

  // Validate local token against DB
  if let Some(ref token) = local_token {
    if token.starts_with("odtk_") {
      match auth_tokens::verify_bearer_token(token) {
        Ok(true) => println!("  Verification: local token is valid"),
        Ok(false) => {
          println!("  Verification: FAILED — local token not recognized by database");
          println!(
            "                The token in hook-forward.json doesn't match any active DB token."
          );
          println!("                Run `orbitdock auth reset` to fix.");
        }
        Err(e) => println!("  Verification: error ({})", e),
      }
    }
  }

  println!();
  Ok(())
}

/// Revoke all tokens, issue a fresh local token, and update hook-forward.json.
pub fn reset_auth() -> anyhow::Result<()> {
  println!();

  // Revoke all existing tokens
  let revoked = auth_tokens::revoke_all_tokens()?;
  if revoked > 0 {
    println!("  Revoked {} existing token(s).", revoked);
  }

  // Issue a fresh local token
  let issued = auth_tokens::issue_token(Some("local"))?;
  super::hook_forward::write_transport_config("http://127.0.0.1:4000", Some(&issued.token))?;

  println!("  New local token issued and saved to hook-forward.json.");
  println!();
  println!("  If you have hooks pointing to a remote server, re-run:");
  println!("    orbitdock install-hooks --server-url <url> --auth-token <token>");
  println!();

  Ok(())
}

pub fn revoke_auth_token(token_id: &str) -> anyhow::Result<()> {
  let revoked = auth_tokens::revoke_token(token_id)?;
  println!();
  if revoked {
    println!("  Revoked token {}", token_id.trim());
  } else {
    println!(
      "  Token {} was not found or already revoked",
      token_id.trim()
    );
  }
  println!();
  Ok(())
}
