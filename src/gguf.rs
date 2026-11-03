//! Minimal GGUF header reader: just enough to learn a vision projector's patch
//! grid, so the resampling thresholds can follow the model instead of being
//! hard-coded for one checkpoint.
//!
//! Only the metadata section is parsed (no tensor data); the file is never read
//! past the tensor-info table. A malformed or truncated file returns `None`
//! rather than failing the capture.

use std::fs::File;
use std::io::{BufReader, Read, Result as IoResult};
use std::path::Path;

/// One visual token covers `patch_px × merge_px` pixels of the input image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisionShape {
    pub patch_px: u32,
    pub merge_px: u32,
}

impl VisionShape {
    pub fn token_px(self) -> f32 {
        (self.patch_px.max(1) * self.merge_px.max(1)) as f32
    }
}

/// Read the patch grid from a GGUF vision projector.
///
/// Recognises the tensor names used by the common multimodal families:
/// `v.patch_embd.weight` / `v.patch_embedding.weight` for the patch embedding
/// and `mm.patch_merger.weight` for the 2×2 merger. A projector without a
/// merger reports `merge_px = 1`.
pub fn vision_shape(path: &Path) -> Option<VisionShape> {
    let file = File::open(path).ok()?;
    read_vision_shape(&mut BufReader::new(file)).ok().flatten()
}

const MAX_STRING: u64 = 1 << 20;
const MAX_DIMS: u32 = 8;
const MAX_TENSORS: u64 = 1_000_000;

fn read_vision_shape<R: Read>(r: &mut R) -> IoResult<Option<VisionShape>> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        return Ok(None);
    }
    let _version = read_u32(r)?;
    let tensor_count = read_u64(r)?;
    let kv_count = read_u64(r)?;
    if tensor_count > MAX_TENSORS || kv_count > MAX_TENSORS {
        return Ok(None);
    }

    for _ in 0..kv_count {
        skip_string(r)?;
        let ty = read_u32(r)?;
        skip_value(r, ty)?;
    }

    let mut patch = None;
    let mut merge = None;
    for _ in 0..tensor_count {
        let name = read_string(r)?;
        let n_dims = read_u32(r)?;
        if n_dims > MAX_DIMS {
            return Ok(None);
        }
        let mut dims = [0u64; MAX_DIMS as usize];
        for slot in dims.iter_mut().take(n_dims as usize) {
            *slot = read_u64(r)?;
        }
        let _dtype = read_u32(r)?;
        let _offset = read_u64(r)?;

        // The merger has shape [merge_h, merge_w, hidden_in, hidden_out];
        // `token_px` needs the *edge* factor, not the area multiple.
        if name.contains("patch_merger") && n_dims >= 2 && merge.is_none() {
            merge = Some(dims[0].max(dims[1]).max(1) as u32);
        // The patch embedding is [patch_w, patch_h, channels, hidden]; the bias
        // that shares the name prefix is 1-D and must not be mistaken for it.
        } else if (name.contains("patch_embd") || name.contains("patch_embedding"))
            && n_dims >= 3
            && dims[0] > 0
            && patch.is_none()
        {
            patch = Some(dims[0] as u32);
        }
    }

    let Some(patch_px) = patch else {
        return Ok(None);
    };
    Ok(Some(VisionShape {
        patch_px,
        merge_px: merge.unwrap_or(1).max(1),
    }))
}

fn read_u32<R: Read>(r: &mut R) -> IoResult<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64<R: Read>(r: &mut R) -> IoResult<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_string<R: Read>(r: &mut R) -> IoResult<String> {
    let len = read_u64(r)?;
    if len > MAX_STRING {
        // A bogus length would make the caller allocate wildly; read nothing
        // and let the caller treat the header as unreadable.
        return Ok(String::new());
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn skip_string<R: Read>(r: &mut R) -> IoResult<()> {
    let len = read_u64(r)?;
    if len > MAX_STRING {
        return Ok(());
    }
    std::io::copy(&mut r.by_ref().take(len), &mut std::io::sink())?;
    Ok(())
}

/// Byte size of a fixed-width GGUF scalar, or `None` for string/array.
fn scalar_size(ty: u32) -> Option<u64> {
    Some(match ty {
        0 | 1 | 7 => 1,
        2 | 3 => 2,
        4..=6 => 4,
        10..=12 => 8,
        _ => return None,
    })
}

fn skip_value<R: Read>(r: &mut R, ty: u32) -> IoResult<()> {
    if let Some(size) = scalar_size(ty) {
        return std::io::copy(&mut r.by_ref().take(size), &mut std::io::sink()).map(|_| ());
    }
    match ty {
        8 => skip_string(r),
        9 => {
            let elem = read_u32(r)?;
            let count = read_u64(r)?;
            if let Some(size) = scalar_size(elem) {
                let total = size.saturating_mul(count);
                return std::io::copy(&mut r.by_ref().take(total), &mut std::io::sink())
                    .map(|_| ());
            }
            // Arrays of strings are rare in projector metadata but legal.
            for _ in 0..count.min(MAX_STRING) {
                skip_string(r)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_size_is_patch_times_merge() {
        let s = VisionShape {
            patch_px: 14,
            merge_px: 4,
        };
        assert_eq!(s.token_px(), 56.0);
        let plain = VisionShape {
            patch_px: 16,
            merge_px: 1,
        };
        assert_eq!(plain.token_px(), 16.0);
    }

    #[test]
    fn a_non_gguf_file_is_rejected_without_panicking() {
        let dir = std::env::temp_dir().join(format!("sofer-gguf-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("not-a-model.gguf");
        std::fs::write(&path, b"not gguf at all").unwrap();
        assert!(vision_shape(&path).is_none());
        let _ = std::fs::remove_file(&path);
    }

    /// Opt-in check against a real projector: CI has no model, so the test
    /// returns early unless `SOFER_MMPROJ` points at one.
    #[test]
    fn reads_a_real_projector_when_one_is_configured() {
        let Ok(path) = std::env::var("SOFER_MMPROJ") else {
            return;
        };
        let shape = vision_shape(Path::new(&path)).expect("projector header");
        assert!(shape.patch_px > 0 && shape.merge_px > 0);
        assert!(shape.token_px() >= 1.0);
        eprintln!("{path}: {shape:?} token_px={}", shape.token_px());
    }

    #[test]
    fn a_truncated_header_is_rejected_without_panicking() {
        let dir = std::env::temp_dir().join(format!("sofer-gguf-trunc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("truncated.gguf");
        // Valid magic, then nothing.
        std::fs::write(&path, b"GGUF").unwrap();
        assert!(vision_shape(&path).is_none());
        let _ = std::fs::remove_file(&path);
    }
}
