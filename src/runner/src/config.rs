// The ETHRCFG v3 configuration record. Specified in spec/ethrcfg.md, which is what a builder
// outside this repository is written against.
//
// The record is NOT part of the stub. A builder turns a bare stub into an application's
// launcher by concatenation — `stub ‖ record ‖ jar` — and the runner finds the record at
// startup by scanning its own executable forwards for the first occurrence of the magic. The
// stub therefore must contain the magic nowhere, which is why `magic()` reassembles it at run
// time from an obfuscated constant rather than holding it as a literal the compiler could
// place in `.rodata` or an instruction immediate. (In v2, where the record was a static inside
// the stub, exactly that happened on x86-64: the verifier's comparand was materialised as a
// `movabs` immediate ahead of the real record, and every builder patched the code instead.)
//
// Layout (3764 bytes total):
//   [0..8]        magic: "ETHRCFG" + format version (3)
//   [8..16]       build_id    (u64 little-endian)
//   [16..18]      java_min    (u16 little-endian)
//   [18..20]      java_pref   (u16 little-endian)
//   [20]          bundle      (0 = jre, 1 = jdk)
//   [21]          flags       (bit 0 = downgrade_permitted, others reserved)
//   [22..32]      reserved    (0)
//   [32..1344]    ml_dsa_44 public key (1312 bytes)
//   [1344..3764]  ml_dsa_44 signature  (2420 bytes; zero in the running
//                 binary, set only by the signer. The verifier zeroes this
//                 region of the incoming .pending binary before recomputing
//                 the signature.)
//
// A stub run bare — no record appended — sees the compiled-in defaults below, which are also
// what a zero field means: Java 21 minimum, 24 preferred, a JRE, build id 0, and an all-zero
// public key, which disables self-upgrade.

use std::path::Path;
use std::sync::OnceLock;

pub const RECORD_LEN: usize        = 3764;
pub const MAGIC_LEN: usize         = 8;
pub const PUBKEY_OFFSET: usize     = 32;
pub const PUBKEY_LEN: usize        = 1312;        // ML-DSA-44 |pk|
pub const SIGNATURE_OFFSET: usize  = 1344;
pub const SIGNATURE_LEN: usize     = 2420;        // ML-DSA-44 |sig|

pub const FLAG_DOWNGRADE_PERMITTED: u8 = 0x01;

pub const DEFAULT_JAVA_MIN: u16  = 21;
pub const DEFAULT_JAVA_PREF: u16 = 24;

// How far into its own file the runner will look for a record before giving up and using the
// defaults. A bare stub is well under a megabyte, and the record immediately follows it; the
// bound only limits the cost of a mis-built file that carries no record at all.
const SCAN_LIMIT: u64 = 16 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;

// `ETHRCFG\x03`, each byte XORed with `OBFUSCATION_KEY`. See the module comment.
const OBFUSCATION_KEY: u8 = 0x5A;
const MAGIC_OBFUSCATED: [u8; MAGIC_LEN] = [
    b'E' ^ OBFUSCATION_KEY, b'T' ^ OBFUSCATION_KEY, b'H' ^ OBFUSCATION_KEY, b'R' ^ OBFUSCATION_KEY,
    b'C' ^ OBFUSCATION_KEY, b'F' ^ OBFUSCATION_KEY, b'G' ^ OBFUSCATION_KEY, 3 ^ OBFUSCATION_KEY,
];

// The magic, reassembled at run time. `black_box` stops the optimiser from folding the XOR
// back into the plaintext constant.
#[inline(never)]
pub fn magic() -> [u8; MAGIC_LEN] {
    let key = core::hint::black_box(OBFUSCATION_KEY);
    let mut out = MAGIC_OBFUSCATED;
    for b in &mut out { *b ^= key; }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildConfig {
    pub build_id:  u64,
    pub java_min:  u16,
    pub java_pref: u16,
    pub bundle:    &'static str,
    pub flags:     u8,
}

impl BuildConfig {
    pub const DEFAULT: BuildConfig = BuildConfig {
        build_id:  0,
        java_min:  DEFAULT_JAVA_MIN,
        java_pref: DEFAULT_JAVA_PREF,
        bundle:    "jre",
        flags:     0,
    };
}

// The record loaded by `load`, or absent when `load` was never called or found nothing.
static RECORD: OnceLock<Option<[u8; RECORD_LEN]>> = OnceLock::new();

// The offset of the first complete record in `bytes`: the first occurrence of the magic that
// has `RECORD_LEN` bytes from its start. A magic too close to the end is not a record.
pub fn find_record(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < RECORD_LEN { return None; }
    let magic = magic();
    bytes[..=bytes.len() - RECORD_LEN]
        .windows(MAGIC_LEN)
        .position(|window| window == magic)
}

pub fn parse(record: &[u8; RECORD_LEN]) -> BuildConfig {
    let build_id      = u64::from_le_bytes(record[8..16].try_into().unwrap());
    let java_min_raw  = u16::from_le_bytes(record[16..18].try_into().unwrap());
    let java_pref_raw = u16::from_le_bytes(record[18..20].try_into().unwrap());
    BuildConfig {
        build_id,
        java_min:  if java_min_raw  == 0 { DEFAULT_JAVA_MIN }  else { java_min_raw },
        java_pref: if java_pref_raw == 0 { DEFAULT_JAVA_PREF } else { java_pref_raw },
        bundle:    if record[20] == 0 { "jre" } else { "jdk" },
        flags:     record[21],
    }
}

// Scan a file forwards for the first record, reading it in chunks that overlap by one byte
// less than the magic so that a magic straddling a chunk boundary is still seen. Returns the
// record, or `None` when the file has none within `SCAN_LIMIT`.
fn scan_file(path: &Path) -> Option<[u8; RECORD_LEN]> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let magic = magic();
    let mut buffer = vec![0u8; CHUNK + MAGIC_LEN - 1];
    let mut carry = 0usize;    // bytes retained from the previous chunk, at the buffer's start
    let mut position: u64 = 0; // file offset of buffer[0]

    while position < SCAN_LIMIT {
        let read = file.read(&mut buffer[carry..]).ok()?;
        if read == 0 { return None; }
        let filled = carry + read;

        if let Some(hit) = buffer[..filled].windows(MAGIC_LEN).position(|w| w == magic) {
            let offset = position + hit as u64;
            let mut record = [0u8; RECORD_LEN];
            file.seek(SeekFrom::Start(offset)).ok()?;
            file.read_exact(&mut record).ok()?;
            return Some(record);
        }

        carry = MAGIC_LEN - 1;
        let keep_from = filled - carry;
        buffer.copy_within(keep_from..filled, 0);
        position += keep_from as u64;
    }

    None
}

// Load the record from the running executable. Called once at startup with the path the
// runner was invoked as (which is also what the JVM is given as the JAR); later calls are
// no-ops. The functions below read whatever was loaded, or the defaults.
pub fn load(path: &Path) -> BuildConfig {
    let record = RECORD.get_or_init(|| {
        let found = scan_file(path);
        crate::debug!("config: record {} in {}", if found.is_some() { "found" } else { "absent" }, path.display());
        found
    });
    record.as_ref().map(parse).unwrap_or(BuildConfig::DEFAULT)
}

pub fn read_config() -> BuildConfig {
    RECORD.get().and_then(|r| r.as_ref()).map(parse).unwrap_or(BuildConfig::DEFAULT)
}

// Snapshot of the running binary's ML-DSA-44 public key. Returned as an owned
// array because the caller may need to forward it across thread or FFI
// boundaries; it's only 1312 bytes.
pub fn public_key() -> [u8; PUBKEY_LEN] {
    let mut out = [0u8; PUBKEY_LEN];
    if let Some(record) = RECORD.get().and_then(|r| r.as_ref()) {
        out.copy_from_slice(&record[PUBKEY_OFFSET..PUBKEY_OFFSET + PUBKEY_LEN]);
    }
    out
}

// True iff the baked-in public key is all zeros — the safe "no signing
// configured" state. A runner in this state rejects every upgrade.
#[cfg(test)]
pub fn public_key_is_unset() -> bool {
    public_key().iter().all(|&b| b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn record(build_id: u64, java_min: u16, java_pref: u16, jdk: bool, flags: u8) -> [u8; RECORD_LEN] {
        let mut r = [0u8; RECORD_LEN];
        r[..MAGIC_LEN].copy_from_slice(&magic());
        r[8..16].copy_from_slice(&build_id.to_le_bytes());
        r[16..18].copy_from_slice(&java_min.to_le_bytes());
        r[18..20].copy_from_slice(&java_pref.to_le_bytes());
        r[20] = if jdk { 1 } else { 0 };
        r[21] = flags;
        r
    }

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("xeq-config-test-{}-{}", std::process::id(), name));
        std::fs::File::create(&path).unwrap().write_all(bytes).unwrap();
        path
    }

    #[test]
    fn magic_is_ethrcfg_v3() {
        assert_eq!(&magic(), b"ETHRCFG\x03");
    }

    #[test]
    fn finds_record_after_prefix_and_ignores_decoy_in_jar() {
        let mut bytes = vec![0xAAu8; 1000];
        bytes.extend_from_slice(&record(42, 17, 21, true, 1));
        let mut jar = vec![0x55u8; 500];
        jar.extend_from_slice(&magic());          // a decoy inside the "jar"
        jar.extend_from_slice(&[0u8; RECORD_LEN]);
        bytes.extend_from_slice(&jar);
        assert_eq!(find_record(&bytes), Some(1000));
    }

    #[test]
    fn magic_too_close_to_end_is_not_a_record() {
        let mut bytes = vec![0u8; 100];
        bytes.extend_from_slice(&magic());
        bytes.extend_from_slice(&[0u8; 100]);
        assert_eq!(find_record(&bytes), None);
    }

    #[test]
    fn fields_round_trip_and_zero_means_default() {
        let parsed = parse(&record(7, 17, 21, true, 1));
        assert_eq!(parsed, BuildConfig { build_id: 7, java_min: 17, java_pref: 21, bundle: "jdk", flags: 1 });
        let defaults = parse(&record(0, 0, 0, false, 0));
        assert_eq!(defaults, BuildConfig::DEFAULT);
    }

    #[test]
    fn scan_finds_record_straddling_a_chunk_boundary() {
        // Place the magic three bytes before the first chunk ends.
        let mut bytes = vec![0x11u8; CHUNK - 3];
        bytes.extend_from_slice(&record(99, 0, 0, false, 0));
        bytes.extend_from_slice(&[0x22u8; 4096]);
        let path = temp_file("straddle", &bytes);
        let found = scan_file(&path).expect("record");
        assert_eq!(parse(&found).build_id, 99);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn scan_without_record_gives_nothing() {
        let path = temp_file("bare", &vec![0x33u8; 3 * CHUNK + 17]);
        assert!(scan_file(&path).is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn scan_beyond_limit_gives_nothing() {
        let mut bytes = vec![0u8; SCAN_LIMIT as usize + 10];
        let start = SCAN_LIMIT as usize + 1;
        bytes.extend_from_slice(&record(5, 0, 0, false, 0));
        bytes[start..start + MAGIC_LEN].copy_from_slice(&magic());
        let path = temp_file("beyond", &bytes);
        assert!(scan_file(&path).is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn scan_reads_the_public_key() {
        let mut r = record(1, 0, 0, false, 0);
        for (i, b) in r[PUBKEY_OFFSET..PUBKEY_OFFSET + PUBKEY_LEN].iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let mut bytes = vec![0x44u8; 777];
        bytes.extend_from_slice(&r);
        let path = temp_file("pubkey", &bytes);
        let found = scan_file(&path).expect("record");
        assert_eq!(&found[PUBKEY_OFFSET..PUBKEY_OFFSET + 8], &[0, 1, 2, 3, 4, 5, 6, 7]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unloaded_runner_reports_defaults_and_no_key() {
        // `RECORD` may or may not have been initialised by another test in this process; only
        // assert what holds either way for a record-free process.
        if RECORD.get().is_none() {
            assert_eq!(read_config(), BuildConfig::DEFAULT);
            assert!(public_key_is_unset());
        }
    }
}
