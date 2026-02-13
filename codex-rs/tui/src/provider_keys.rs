use std::io::{self, BufRead, Write};

/// Load previously saved API keys from `~/.codex/provider_keys.env`.
///
/// Reads the file and sets any env vars that are not already set.
/// If a var appears multiple times (append-mode), the last value wins.
/// Called at startup before `ensure_api_key_available`.
#[allow(clippy::print_stderr)]
pub fn load_provider_keys() {
    let codex_home = match codex_core::config::find_codex_home() {
        Ok(h) => h,
        Err(_) => return,
    };
    let keys_file = codex_home.join("provider_keys.env");
    let file = match std::fs::File::open(&keys_file) {
        Ok(f) => f,
        Err(_) => return, // File doesn't exist yet — that's fine
    };

    // Collect all key=value pairs, last value wins for duplicates
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in io::BufReader::new(file).lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();
        // Parse: export KEY="value" or KEY="value" or KEY=value
        let kv = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        if let Some((key, raw_val)) = kv.split_once('=') {
            let key = key.trim();
            let val = raw_val.trim().trim_matches('"').trim().to_string();
            if !key.is_empty() && !val.is_empty() {
                entries.push((key.to_string(), val));
            }
        }
    }

    // Set env vars (last entry wins for duplicates), only if not already set
    // Process in reverse to find the last value for each key
    let mut seen = std::collections::HashSet::new();
    for (key, val) in entries.into_iter().rev() {
        if seen.insert(key.clone()) {
            if std::env::var(&key).ok().filter(|v| !v.trim().is_empty()).is_none() {
                // SAFETY: Called during single-threaded CLI startup.
                unsafe { std::env::set_var(&key, &val) };
            }
        }
    }
}

/// Ensure the API key for the given environment variable is available.
///
/// If the key is already set in the environment, this is a no-op.
/// Otherwise, it prompts the user interactively, sets the env var for the
/// current process, and persists it to `~/.codex/provider_keys.env`.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn ensure_api_key_available(
    env_var: &str,
    provider_name: &str,
    signup_url: &str,
) -> io::Result<()> {
    // Already set? Nothing to do.
    if std::env::var(env_var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some()
    {
        return Ok(());
    }

    eprintln!(
        "\nNo {provider_name} API key found (env var: {env_var}).\n\
         Get one at: {signup_url}\n"
    );
    eprint!("Enter your {provider_name} API key: ");
    io::stderr().flush()?;

    let mut api_key = String::new();
    io::stdin().read_line(&mut api_key)?;
    let api_key = api_key.trim().to_string();

    if api_key.is_empty() {
        return Err(io::Error::other(format!(
            "{provider_name} API key is required. Set {env_var} or pass it when prompted."
        )));
    }

    // Set for the current process.
    // SAFETY: Called during single-threaded CLI startup before any async
    // runtime threads inspect environment variables.
    unsafe { std::env::set_var(env_var, &api_key) };

    // Persist to ~/.codex/provider_keys.env so future sessions pick it up
    // automatically when the user sources this file or uses direnv.
    if let Ok(codex_home) = codex_core::config::find_codex_home() {
        let keys_file = codex_home.join("provider_keys.env");
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&keys_file)
        {
            Ok(mut f) => {
                writeln!(f, "export {env_var}=\"{api_key}\"")?;
                // Best-effort chmod 600.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &keys_file,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
                eprintln!(
                    "API key saved to {}. Source this file in your shell profile to persist it.",
                    keys_file.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "Warning: could not save API key to {}: {e}",
                    keys_file.display()
                );
            }
        }
    }

    Ok(())
}
