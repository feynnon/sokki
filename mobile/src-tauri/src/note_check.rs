//! The background check for finished meeting notes (`NoteCheck.kt`): while
//! the app is closed, Android runs a job about every 15 minutes that asks
//! the paired Mac about the recordings it is still working on, and shows
//! "Note ready" when one is done.
//!
//! The job calls `nativeStatus` over JNI with the app's data folder (where
//! `link.rs` keeps this phone's key and the pairing) and the Mac's meeting
//! ids. One attempt per id, at most 30 s in all; when the Mac is off every
//! answer is an error and the job tries again next time.

use std::path::Path;
use std::time::Duration;

use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use serde_json::{json, Value};
use sokki_link::iroh::EndpointId;
use sokki_link::Request;

/// This phone's key and the paired Mac (`link.rs`).
fn pairing(dir: &Path) -> Result<(sokki_link::iroh::SecretKey, EndpointId), String> {
    let key = std::fs::read_to_string(dir.join("link_key.txt")).map_err(|_| "no key")?;
    let key = sokki_link::secret_from_hex(key.trim()).map_err(|_| "bad key")?;
    let p: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("pairing.json")).map_err(|_| "not paired")?,
    )
    .map_err(|_| "bad pairing")?;
    let mac = p["mac"].as_str().ok_or("bad pairing")?;
    Ok((key, mac.parse().map_err(|_| "bad pairing")?))
}

fn fail(ids: &[String], e: &str) -> Vec<Value> {
    ids.iter().map(|id| json!({"id": id, "error": e})).collect()
}

/// `[{id, status…}]` for each id, or `[{id, error}]`.
async fn statuses(dir: &Path, ids: Vec<String>) -> Vec<Value> {
    let (key, mac) = match pairing(dir) {
        Ok(p) => p,
        Err(e) => return fail(&ids, &e),
    };
    let ep = match sokki_link::bind(key).await {
        Ok(ep) => ep,
        Err(e) => return fail(&ids, &format!("{e:#}")),
    };
    let mut out = Vec::new();
    for id in &ids {
        let v = match sokki_link::call(&ep, mac, &Request::Status { id: id.clone() }, &[]).await {
            Ok(v) => v,
            Err(e) => json!({"id": id, "error": format!("{e:#}")}),
        };
        out.push(v);
    }
    ep.close().await;
    out
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_mobile_NoteCheck_nativeStatus(
    mut env: JNIEnv,
    _class: JClass,
    data_dir: JString,
    ids: JString,
) -> jstring {
    let dir: String = env.get_string(&data_dir).map(String::from).unwrap_or_default();
    let ids: String = env.get_string(&ids).map(String::from).unwrap_or_default();
    let ids: Vec<String> = serde_json::from_str(&ids).unwrap_or_default();
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(_) => return std::ptr::null_mut(),
    };
    let all = ids.clone();
    let out = rt.block_on(async {
        tokio::time::timeout(Duration::from_secs(30), statuses(Path::new(&dir), ids))
            .await
            .unwrap_or_else(|_| fail(&all, "timeout"))
    });
    env.new_string(Value::Array(out).to_string())
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
