//! Merges matra-hook.exe without removing another product's hook commands.
//! Legacy migration is restricted to our own Programs/MatraNotch directory.

use serde_json::{json, Value};
use std::path::PathBuf;

/// (Claude Code event name, whether it needs a matcher, the internal event reported to Codenotch)
const WIRING: &[(&str, bool, &str)] = &[
    ("SessionStart", false, "session_start"),
    ("UserPromptSubmit", false, "running"),
    ("PreToolUse", true, "running"),
    ("PostToolUse", true, "running"),
    ("Notification", false, "attention"),
    ("Stop", false, "done"),
    ("SessionEnd", false, "session_end"),
];

fn settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
}

fn own_command(h: &Value) -> bool {
    let cmd=h["command"].as_str().unwrap_or("").trim_start();
    let exe=if let Some(s)=cmd.strip_prefix('"') {s.split('"').next().unwrap_or("")}
        else {cmd.split_whitespace().next().unwrap_or("")};
    let exe=exe.replace('/', "\\").to_lowercase();
    let name=exe.rsplit('\\').next().unwrap_or("");
    name=="matra-hook.exe" || (name=="codenotch-hook.exe" && exe.contains("\\programs\\matranotch\\"))
}

fn strip_ours(arr: Vec<Value>) -> Vec<Value> {
    arr.into_iter().filter_map(|mut entry| {
        if let Some(hs)=entry["hooks"].as_array_mut() {
            let before=hs.len();hs.retain(|h|!own_command(h));
            if before>0&&hs.is_empty(){return None;}
        }
        Some(entry)
    }).collect()
}

fn load(path: &PathBuf) -> Result<Value,String> {
    if !path.exists(){return Ok(json!({}));}
    let text=std::fs::read_to_string(path).map_err(|e|e.to_string())?;
    let value:Value=serde_json::from_str(&text).map_err(|e|format!("Settings JSON is invalid; left untouched: {e}"))?;
    if !value.is_object(){return Err("Settings must be a JSON object; left untouched".into());}
    Ok(value)
}

fn backup_and_write(path: &PathBuf, root: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        std::fs::copy(path, path.with_extension(format!("json.matra-bak-{ts}"))).map_err(|e|format!("Backup failed: {e}"))?;
    }
    let txt = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    let pending=path.with_extension(format!("json.matra-pending-{}",std::process::id()));
    std::fs::write(&pending, txt).map_err(|e| e.to_string())?;
    std::fs::rename(&pending,path).map_err(|e|format!("Could not replace settings; original and backup preserved: {e}"))
}

pub fn is_installed() -> bool {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t|serde_json::from_str::<Value>(&t).ok())
        .map(|root|WIRING.iter().all(|(event,_,_)|root["hooks"][*event].as_array().is_some_and(|entries|
            entries.iter().any(|e|e["hooks"].as_array().is_some_and(|hs|hs.iter().any(own_command))))))
        .unwrap_or(false)
}

pub fn diagnostics() -> String {
    let Some(path)=settings_path() else{return "hooks: no user directory\n".into()};
    let root=match load(&path){Ok(v)=>v,Err(e)=>return format!("hooks: {e}\n")};
    let mut out=String::from("hook routing (Matra entries vs preserved other commands):\n");
    for (event,_,_) in WIRING {
        let commands:Vec<&Value>=root["hooks"][*event].as_array().into_iter().flatten()
            .flat_map(|e|e["hooks"].as_array().into_iter().flatten()).collect();
        let ours=commands.iter().filter(|h|own_command(h)).count();
        out+=&format!("  {event}: Matra={ours}, other={}\n",commands.len()-ours);
    }
    out
}

pub fn install() -> Result<String, String> {
    let path = settings_path().ok_or("cannot find the user directory")?;
    let hook_exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("cannot locate the program directory")?
        .join("matra-hook.exe");
    if !hook_exe.exists() {
        return Err(format!("missing {}", hook_exe.display()));
    }

    let mut root = load(&path)?;
    if !root.is_object() {
        root = json!({});
    }
    if !root["hooks"].is_null()&&!root["hooks"].is_object(){return Err("Invalid hooks object; left untouched".into());}
    if !root["hooks"].is_object() {
        root["hooks"] = json!({});
    }

    for (event, need_matcher, internal) in WIRING {
        let arr = root["hooks"][*event].as_array().cloned().unwrap_or_default();
        // Remove our own older entries first
        if !root["hooks"][*event].is_null()&&!root["hooks"][*event].is_array(){return Err(format!("Invalid {event} hook list; left untouched"));}
        let mut arr=strip_ours(arr);
        let cmd = format!("\"{}\" {}", hook_exe.display(), internal);
        let mut entry = json!({
            "hooks": [{ "type": "command", "command": cmd, "timeout": 5 }]
        });
        if *need_matcher {
            entry["matcher"] = json!("*");
        }
        arr.push(entry);
        root["hooks"][*event] = json!(arr);
    }

    backup_and_write(&path, &root)?;
    Ok(format!("wrote {} ({} events)", path.display(), WIRING.len()))
}

pub fn uninstall() -> Result<String, String> {
    let path = settings_path().ok_or("cannot find the user directory")?;
    if !path.exists() {
        return Ok("settings.json does not exist, nothing to uninstall".into());
    }
    let mut root = load(&path)?;
    let Some(hooks) = root["hooks"].as_object_mut() else {
        return Ok("no hooks configuration found".into());
    };
    let mut removed = 0;
    for (_, v) in hooks.iter_mut() {
        if let Some(arr) = v.as_array() {
            let filtered=strip_ours(arr.clone());
            removed += arr.len() - filtered.len();
            *v = json!(filtered);
        }
    }
    backup_and_write(&path, &root)?;
    Ok(format!("removed {removed} Matra hook group(s); foreign hooks preserved"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_and_mixed_hooks_survive() {
        let foreign=json!({"type":"command","command":"\"D:\\MATRA-V2\\codenotch-hook.exe\" running"});
        let own=json!({"type":"command","command":"\"C:\\Apps\\matra-hook.exe\" running"});
        assert!(!own_command(&foreign));assert!(own_command(&own));
        let mixed=json!({"matcher":"*","hooks":[foreign.clone(),own]});
        assert_eq!(strip_ours(vec![mixed]),vec![json!({"matcher":"*","hooks":[foreign]})]);
        assert!(!own_command(&json!({"command":"echo matra-hook.exe"})));
    }
    #[test]
    fn only_our_legacy_path_is_migrated() {
        assert!(own_command(&json!({"command":"\"C:\\Users\\x\\AppData\\Local\\Programs\\MatraNotch\\v1\\codenotch-hook.exe\" running"})));
        assert!(!own_command(&json!({"command":"\"C:\\Apps\\codenotch-hook.exe\" running"})));
    }
}
