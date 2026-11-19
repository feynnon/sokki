//! The model files Sokki needs, downloaded once with progress and checked
//! against their SHA-256. This is one of the few places Sokki uses the
//! network (see ARCHITECTURE.md, "Audio stays on the device").

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct File {
    /// Path under the models folder.
    pub path: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

/// Parakeet TDT 0.6B v3, Q8_0 (Handy's catalog quant): speech to text in 25
/// European languages.
pub const PARAKEET: File = File {
    path: "parakeet-tdt-0.6b-v3-Q8_0.gguf",
    url: "https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf/resolve/85ac09ea12fc4b1112fa76810059364bc6adc9de/parakeet-tdt-0.6b-v3-Q8_0.gguf",
    sha256: "5859f77944efcd8eafa23a6350731960b2b55b2203df51f319665c807d802cc7",
    size: 739_508_576,
};

fn from_diarize(f: diarize::models::ModelFile) -> File {
    File {
        path: f.path,
        url: f.url,
        sha256: f.sha256,
        size: f.size,
    }
}

/// Everything: speech, speakers, voices.
pub fn all() -> Vec<File> {
    let cfg = diarize::DiarizeConfig::new(crate::paths::models_dir());
    vec![
        PARAKEET,
        from_diarize(diarize::models::SORTFORMER_Q8_0),
        from_diarize(cfg.embedding_model.file()),
    ]
}

pub fn path(file: &File) -> PathBuf {
    crate::paths::models_dir().join(file.path)
}

/// On disk with the right size (hashing 700 MB at every start is wasteful;
/// a fresh download is always hash-checked).
pub fn present(file: &File) -> bool {
    std::fs::metadata(path(file)).is_ok_and(|m| m.len() == file.size)
}

pub fn all_present() -> bool {
    all().iter().all(present)
}

/// Downloads `file` unless present. `progress(done, total)` is called as
/// bytes arrive.
pub fn ensure(file: &File, mut progress: impl FnMut(u64, u64)) -> Result<PathBuf> {
    let dest = path(file);
    if present(file) {
        return Ok(dest);
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = dest.with_extension("part");
    let result = download(file, &tmp, &mut progress);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &dest).with_context(|| format!("saving {}", dest.display()))?;
    Ok(dest)
}

fn download(file: &File, tmp: &Path, progress: &mut impl FnMut(u64, u64)) -> Result<()> {
    let mut resp = ureq::get(file.url)
        .call()
        .with_context(|| format!("downloading {}", file.url))?;
    let mut reader = resp.body_mut().as_reader();
    let mut out = std::fs::File::create(tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    let mut last = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        // About every 4 MB, so the UI moves without flooding it.
        if done - last >= 4 << 20 {
            last = done;
            progress(done, file.size);
        }
    }
    out.flush()?;
    progress(done, file.size);
    let hash = format!("{:x}", hasher.finalize());
    if hash != file.sha256 {
        bail!("{}: checksum mismatch (got {hash})", file.path);
    }
    Ok(())
}
