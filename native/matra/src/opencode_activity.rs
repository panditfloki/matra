//! Metadata-only OpenCode activity. A bridge file belongs to a live process instance,
//! not just a PID that Windows may have reused since a crash.
use crate::activity::Activity;
use crate::providers_plus::custom::CustomEndpoint;
use serde_json::Value;
use std::collections::HashSet;
use std::io::Read;

fn parse(root: &Value, endpoints: &[CustomEndpoint], now: u64, live: impl Fn(u32,u64)->bool) -> Vec<Activity> {
    if root["version"].as_u64()!=Some(1) { return vec![]; }
    let Some(pid)=root["pid"].as_u64().and_then(|v| u32::try_from(v).ok()).filter(|v| *v>0) else { return vec![] };
    let Some(started)=root["startedAt"].as_u64() else { return vec![] };
    if !live(pid,started) { return vec![]; }
    let mut seen=HashSet::new();
    root["sessions"].as_array().into_iter().flatten().take(64).filter_map(|session| {
        if !session["parentID"].is_null() { return None; }
        let id=session["sessionID"].as_str()?;
        if id.is_empty() || !seen.insert(id) { return None; }
        let state=session["state"].as_str()?;
        if !matches!(state,"busy"|"waiting") { return None; }
        let provider=session["providerID"].as_str().unwrap_or("");
        let model=session["modelID"].as_str().unwrap_or("");
        let mapped=endpoints.iter().find(|endpoint| endpoint.is_enabled
            && endpoint.open_code_provider_id.as_deref().is_some_and(|id| !id.is_empty() && id==provider)
            && endpoint.open_code_model_id.as_deref().is_none_or(|id| id.is_empty() || id==model));
        let provider=mapped.map(|e| format!("custom:{}",e.id)).unwrap_or_else(|| "opencode".into());
        Some(Activity {provider,state:state.into(),name:"OpenCode".into(),
            detail:if state=="waiting" {"Needs your input".into()}else{"Working".into()},
            since:session["since"].as_u64().unwrap_or(now).min(now)})
    }).collect()
}

#[cfg(windows)]
fn live_process(pid:u32,started:u64)->bool {
    use windows::Win32::Foundation::{CloseHandle,FILETIME};
    use windows::Win32::System::Threading::{OpenProcess,GetProcessTimes,GetExitCodeProcess,PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let Ok(handle)=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid) else { return false; };
        let (mut created,mut exited,mut kernel,mut user)=(FILETIME::default(),FILETIME::default(),FILETIME::default(),FILETIME::default());
        let mut code=0u32;
        let okay=GetExitCodeProcess(handle,&mut code).is_ok() && code==259
            && GetProcessTimes(handle,&mut created,&mut exited,&mut kernel,&mut user).is_ok();
        let _=CloseHandle(handle);
        let at=(((created.dwHighDateTime as u64)<<32) | created.dwLowDateTime as u64)/10_000;
        okay && at.saturating_sub(11_644_473_600_000).abs_diff(started)<=2_000
    }
}
#[cfg(not(windows))]
fn live_process(_pid:u32,_started:u64)->bool { false }

pub fn read() -> Vec<Activity> {
    let Some(parent)=crate::config::config_path().parent().map(std::path::Path::to_path_buf) else {return vec![]};
    let endpoints=crate::providers_plus::list_custom_endpoints();
    let mut found=Vec::new();
    for entry in std::fs::read_dir(parent.join("OpenCode")).into_iter().flatten().flatten().take(64) {
        let path=entry.path();
        if path.extension().and_then(|v| v.to_str())!=Some("json") || entry.file_type().is_ok_and(|v| v.is_symlink()) {continue;}
        let Ok(file)=std::fs::File::open(path) else {continue};
        let mut bytes=Vec::new();
        if file.take(1_048_577).read_to_end(&mut bytes).is_err() || bytes.len()>1_048_576 {continue;}
        let Ok(root)=serde_json::from_slice(&bytes) else {continue};
        found.extend(parse(&root,&endpoints,crate::now_ms(),live_process));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->Value {serde_json::json!({"version":1,"pid":12,"startedAt":100,"sessions":[
        {"sessionID":"a","parentID":null,"providerID":"work","modelID":"m","state":"busy","since":120,"title":"private prompt"},
        {"sessionID":"child","parentID":"a","state":"busy"},
        {"sessionID":"idle","parentID":null,"state":"idle"}]})}
    #[test]
    fn ignores_dead_processes_children_and_prompt_titles() {
        assert!(parse(&fixture(),&[],200,|_,_|false).is_empty());
        let value=parse(&fixture(),&[],200,|pid,at|pid==12&&at==100);
        assert_eq!(value.len(),1);assert_eq!(value[0].provider,"opencode");
        assert!(!serde_json::to_string(&value).unwrap().contains("private prompt"));
    }
    #[test]
    fn only_an_explicit_provider_and_model_mapping_claims_custom_activity() {
        let endpoint:CustomEndpoint=serde_json::from_value(serde_json::json!({"id":"work","name":"Work","base_url":"https://example.invalid","open_code_provider_id":"work","open_code_model_id":"m"})).unwrap();
        assert_eq!(parse(&fixture(),&[endpoint.clone()],200,|_,_|true)[0].provider,"custom:work");
        let mut other=endpoint;other.open_code_model_id=Some("different".into());
        assert_eq!(parse(&fixture(),&[other],200,|_,_|true)[0].provider,"opencode");
    }
}
