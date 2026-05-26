use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use sha1::{Digest as Sha1Digest, Sha1};
use sha2::Sha512;

use crate::models::FileHash;

const BLOCK_SIZE: usize = 4096;

pub fn compute_file_hashes(path: &Path) -> anyhow::Result<FileHash> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut sha512 = Sha512::new();
    let mut sha1 = Sha1::new();
    let mut buffer = [0u8; BLOCK_SIZE];
    let mut fingerprint: i64 = 1;

    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        sha512.update(&buffer[..n]);
        sha1.update(&buffer[..n]);
        fingerprint = curseforge_fingerprint_block(&buffer[..n], fingerprint);
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown.jar")
        .to_string();

    Ok(FileHash {
        path: path.to_string_lossy().to_string(),
        file_name,
        sha512: hex::encode(sha512.finalize()),
        sha1: hex::encode(sha1.finalize()),
        fingerprint,
    })
}

fn curseforge_fingerprint_block(data: &[u8], seed: i64) -> i64 {
    murmur_hash2_64(data, seed as u64) as i64
}

fn murmur_hash2_64(data: &[u8], seed: u64) -> u64 {
    const M: u64 = 0xc6a4_a793_5bd1_e995;
    const R: u32 = 47;

    let mut h = seed ^ ((data.len() as u64).wrapping_mul(M));
    let mut i = 0;

    while i + 8 <= data.len() {
        let mut k = u64::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
        ]);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h ^= k;
        h = h.wrapping_mul(M);
        i += 8;
    }

    let remainder = &data[i..];
    match remainder.len() {
        7 => {
            h ^= (remainder[6] as u64) << 48;
            h ^= (remainder[5] as u64) << 40;
            h ^= (remainder[4] as u64) << 32;
            h ^= (remainder[3] as u64) << 24;
            h ^= (remainder[2] as u64) << 16;
            h ^= (remainder[1] as u64) << 8;
            h ^= remainder[0] as u64;
            h = h.wrapping_mul(M);
        }
        6 => {
            h ^= (remainder[5] as u64) << 40;
            h ^= (remainder[4] as u64) << 32;
            h ^= (remainder[3] as u64) << 24;
            h ^= (remainder[2] as u64) << 16;
            h ^= (remainder[1] as u64) << 8;
            h ^= remainder[0] as u64;
            h = h.wrapping_mul(M);
        }
        5 => {
            h ^= (remainder[4] as u64) << 32;
            h ^= (remainder[3] as u64) << 24;
            h ^= (remainder[2] as u64) << 16;
            h ^= (remainder[1] as u64) << 8;
            h ^= remainder[0] as u64;
            h = h.wrapping_mul(M);
        }
        4 => {
            h ^= (remainder[3] as u64) << 24;
            h ^= (remainder[2] as u64) << 16;
            h ^= (remainder[1] as u64) << 8;
            h ^= remainder[0] as u64;
            h = h.wrapping_mul(M);
        }
        3 => {
            h ^= (remainder[2] as u64) << 16;
            h ^= (remainder[1] as u64) << 8;
            h ^= remainder[0] as u64;
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= (remainder[1] as u64) << 8;
            h ^= remainder[0] as u64;
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= remainder[0] as u64;
            h = h.wrapping_mul(M);
        }
        _ => {}
    }

    h ^= h >> R;
    h = h.wrapping_mul(M);
    h ^= h >> R;
    h
}

/// Hash multiple jar files in parallel (I/O bound).
pub fn compute_file_hashes_parallel(paths: &[std::path::PathBuf]) -> Vec<FileHash> {
    if paths.is_empty() {
        return Vec::new();
    }
    if paths.len() == 1 {
        return compute_file_hashes(&paths[0])
            .ok()
            .into_iter()
            .collect();
    }

    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .iter()
            .map(|path| scope.spawn(|| compute_file_hashes(path).ok()))
            .collect();

        let mut results = Vec::with_capacity(paths.len());
        for (path, handle) in paths.iter().zip(handles) {
            if let Ok(Some(hash)) = handle.join() {
                results.push(hash);
            } else {
                eprintln!("hash failed: {}", path.display());
            }
        }
        results
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn murmur_empty_with_seed_one() {
        assert_eq!(murmur_hash2_64(&[], 1), 0);
    }
}
