//! Model files: where they come from, where they live, and fetching them.
//!
//! Models are never bundled or committed. They are downloaded on first use
//! into a caller-chosen directory (`DiarizeConfig::models_dir`), verified by
//! SHA-256, and reused afterwards. Pass `allow_download: false` to only use
//! files already present (e.g. when the app manages downloads itself; the
//! expected file names and URLs are public here for that purpose).

#[cfg(feature = "download")]
use std::fs;
#[cfg(feature = "download")]
use std::io::{Read, Write};
#[cfg(feature = "download")]
use std::path::{Path, PathBuf};

#[cfg(feature = "download")]
use anyhow::{bail, Context, Result};
#[cfg(feature = "download")]
use sha2::{Digest, Sha256};

/// One downloadable model file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    /// Path relative to the models directory.
    pub path: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

/// pyannote segmentation 3.0, ONNX export by sherpa-onnx (MIT).
pub const PYANNOTE_SEGMENTATION: ModelFile = ModelFile {
    path: "sherpa-onnx-pyannote-segmentation-3-0/model.onnx",
    url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx",
    sha256: "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079",
    size: 5_992_913,
};

/// NVIDIA Streaming Sortformer 4spk v2.1, GGUF Q8_0 (the quant Handy's
/// catalog uses), run by transcribe-cpp.
pub const SORTFORMER_Q8_0: ModelFile = ModelFile {
    path: "diar_streaming_sortformer_4spk-v2.1-Q8_0.gguf",
    url: "https://huggingface.co/handy-computer/diar_streaming_sortformer_4spk-v2.1-gguf/resolve/ae4afbb5c3d33b71cf2dbf600022b655ee706dd0/diar_streaming_sortformer_4spk-v2.1-Q8_0.gguf",
    sha256: "a5dacdc650790266c7a362e54e6bf51952015487edaa606c4e11632bc32442a9",
    size: 139_310_336,
};

/// Speaker-embedding models usable with sherpa-onnx. All produce
/// embeddings compared by cosine similarity; embeddings from different
/// models are not comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbeddingModel {
    /// WeSpeaker ResNet34-LM trained on VoxCeleb (256-d, 26 MB).
    WespeakerResnet34Lm,
    /// WeSpeaker CAM++-LM trained on VoxCeleb (512-d, 29 MB).
    WespeakerCamppLm,
    /// 3D-Speaker ERes2Net trained on VoxCeleb (192-d, 26 MB).
    ERes2NetVoxceleb,
    /// NVIDIA NeMo TitaNet small (192-d, 40 MB).
    TitanetSmall,
    /// 3D-Speaker CAM++ "advanced", Chinese + English data (192-d, 28 MB).
    CamppZhEnAdvanced,
}

impl EmbeddingModel {
    pub const ALL: [EmbeddingModel; 5] = [
        EmbeddingModel::WespeakerResnet34Lm,
        EmbeddingModel::WespeakerCamppLm,
        EmbeddingModel::ERes2NetVoxceleb,
        EmbeddingModel::TitanetSmall,
        EmbeddingModel::CamppZhEnAdvanced,
    ];

    /// Stable id, e.g. for storing next to voice profiles.
    pub fn id(self) -> &'static str {
        self.file().path.trim_end_matches(".onnx")
    }

    /// Inverse of [`id`](Self::id).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    pub fn file(self) -> ModelFile {
        macro_rules! m {
            ($name:literal, $sha:literal, $size:literal) => {
                ModelFile {
                    path: $name,
                    url: concat!(
                        "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/",
                        $name
                    ),
                    sha256: $sha,
                    size: $size,
                }
            };
        }
        match self {
            EmbeddingModel::WespeakerResnet34Lm => m!(
                "wespeaker_en_voxceleb_resnet34_LM.onnx",
                "e9848563da86f263117134dfd7ad63c92355b37de492b55e325400c9d9c39012",
                26530550
            ),
            EmbeddingModel::WespeakerCamppLm => m!(
                "wespeaker_en_voxceleb_CAM++_LM.onnx",
                "e197af7e9d473030cf486b3124149a19bf37014d0e4485e4c70c483b0ec10cb2",
                29292687
            ),
            EmbeddingModel::ERes2NetVoxceleb => m!(
                "3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx",
                "c59158379255ad66e161679cca6af8d52d51e389e3224ab7d7a7baae295c2db5",
                26485263
            ),
            EmbeddingModel::TitanetSmall => m!(
                "nemo_en_titanet_small.onnx",
                "ad4a1802485d8b34c722d2a9d04249662f2ece5d28a7a039063ca22f515a789e",
                40257283
            ),
            EmbeddingModel::CamppZhEnAdvanced => m!(
                "3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx",
                "aa3cfc16963a10586a9393f5035d6d6b57e98d358b347f80c2a30bf4f00ceba2",
                28281164
            ),
        }
    }
}

/// Returns the local path of `file` under `dir`, downloading and verifying
/// it first when it is missing and `allow_download` is set.
///
/// A present file is trusted if its size matches (hashing 140 MB on every
/// start is wasteful); a fresh download is always hash-checked.
#[cfg(feature = "download")]
pub fn ensure(dir: &Path, file: &ModelFile, allow_download: bool) -> Result<PathBuf> {
    let dest = dir.join(file.path);
    if let Ok(meta) = fs::metadata(&dest) {
        if meta.len() == file.size {
            return Ok(dest);
        }
    }
    if !allow_download {
        bail!(
            "model file {} is missing (download it from {} or enable downloads)",
            dest.display(),
            file.url
        );
    }
    download(file, &dest)?;
    Ok(dest)
}

#[cfg(feature = "download")]
fn download(file: &ModelFile, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    // Unique temp name so parallel callers do not clobber each other.
    let tmp = dest.with_extension(format!(
        "part{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let result = (|| -> Result<()> {
        let resp = ureq::get(file.url)
            .call()
            .with_context(|| format!("download {}", file.url))?;
        let mut reader = resp.into_reader();
        let mut out =
            fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        out.flush()?;
        let got = hex(&hasher.finalize());
        if got != file.sha256 {
            bail!(
                "checksum mismatch for {}: expected {}, got {got}",
                file.url,
                file.sha256
            );
        }
        fs::rename(&tmp, dest).with_context(|| format!("move into {}", dest.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(feature = "download")]
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_file_stems() {
        assert_eq!(
            EmbeddingModel::WespeakerResnet34Lm.id(),
            "wespeaker_en_voxceleb_resnet34_LM"
        );
        for m in EmbeddingModel::ALL {
            assert!(m.file().url.ends_with(m.file().path));
            assert_eq!(m.file().sha256.len(), 64);
            assert_eq!(EmbeddingModel::from_id(m.id()), Some(m));
        }
    }

    #[cfg(feature = "download")]
    #[test]
    fn missing_file_without_download_errors() {
        let dir = std::env::temp_dir().join("diarize-models-missing-test");
        let err = ensure(&dir, &PYANNOTE_SEGMENTATION, false).unwrap_err();
        assert!(err.to_string().contains("missing"), "{err}");
    }
}
