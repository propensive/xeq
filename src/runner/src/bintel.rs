//! The subset of BinTEL the launcher protocol needs — §4 varints, §6.1 framing, the §7.1
//! node forms and the §8.2 palimpsest signature — written against one fixed schema,
//! `ethereal-launcher`, whose TEL text is `spec/ethereal-launcher.tel`, the contract between
//! this runner and any daemon.
//!
//! The schema's keyword order is compiled in: the document root has a single `select Message`
//! member, so a message is the root node (child count 1) containing one variant node whose
//! keyword index is the variant's position in the select, followed by the variant record's
//! fields in declaration order. Fields are scalars (index, byte length, UTF-8 bytes) or flags
//! (index alone).
//!
//! The schema is a *base* and, in time, *layers* (TEL §20.3): a layer appends optional members
//! to existing records, so the base's keyword indices never move and a layer's fields take the
//! indices after them. A composition — the base with the first `depth − 1` layers — is what a
//! document is written under, and its signature (BinTEL §8.2, a palimpsest of the components'
//! hashes) travels in every frame. Which composition an invocation uses is settled before its
//! first connection from the daemon's acceptance (`acceptance.rs`); a document carrying any
//! other signature is rejected before a field is read, so a runner and a daemon that disagree
//! fail loudly rather than misread each other.
//!
//! No general TEL machinery is here — no schema parsing, no hashing, no BASE-256 — because
//! the runner is a size-optimised launcher and the contract is fixed at build time. The
//! component hashes are pinned constants, and a signature is a few XORs over them.

use std::io::{self, Read};

/// §6.1 field 1: the external-schema magic number, `βτελ` in BASE-256.
pub const MAGIC: [u8; 4] = [0xB2, 0xC4, 0xB5, 0xBB];

/// The BLAKE3-256 value hash of the `ethereal-launcher` base schema — the schema with every
/// `layer` removed (BinTEL §8.1) — pinned here and in the daemon's tests. The base alone has
/// the 33-byte signature `e50b7e82…1e59e5`.
pub const BASE: [u8; 32] = [
    0xe5, 0x0b, 0x7e, 0x82, 0xc1, 0x1b, 0x06, 0x78, 0x3d, 0xaf, 0xa8, 0xa2, 0xec, 0xc4, 0xe3,
    0x5f, 0x7b, 0xa3, 0x10, 0x44, 0xec, 0xd3, 0x8f, 0xc5, 0xd9, 0xfe, 0x9e, 0x47, 0xa7, 0xc1,
    0x1e, 0x59,
];

/// The kind of a record field, per the schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind { Scalar, Flag }

/// One layer of `spec/ethereal-launcher.tel`: its value hash (BinTEL §8.1) and the members
/// it appends, in declaration order, to each variant's record. A layer only ever appends
/// optional members (see `spec/COMPATIBILITY.md`), so the fields of the base and of earlier
/// layers keep their indices and this layer's take the next ones.
pub struct Layer {
    pub hash: [u8; 32],
    pub fields: &'static [(u64, Kind)],
}

/// The layers of the schema, in the order the schema file declares them. The runner's
/// library is exactly the chain of prefixes of this list: the base, the base with the first
/// layer, and so on.
pub static LAYERS: &[Layer] = &[];

/// The hashes of the base and every layer, in composition order.
pub fn chain() -> Vec<[u8; 32]> {
    let mut hashes = vec![BASE];
    hashes.extend(LAYERS.iter().map(|layer| layer.hash));
    hashes
}

/// The BinTEL-pinned cadence byte of a palimpsest signature (§8.2 step 4).
pub const CADENCE: u8 = 0x79;

/// §8.2: the palimpsest signature of an ordered sequence of component hashes. The hashes are
/// XORed into a body at offsets 0, 4, 6, 8, …, and a trailer byte makes the whole XOR to the
/// cadence byte. 33 bytes for a base alone, then two more per further component.
pub fn palimpsest(hashes: &[[u8; 32]]) -> Vec<u8> {
    let n = hashes.len();
    let length = if n <= 1 { 32 } else { 36 + 2 * (n - 2) };
    let mut body = vec![0u8; length];
    for (i, hash) in hashes.iter().enumerate() {
        let offset = if i == 0 { 0 } else { 4 + 2 * (i - 1) };
        for (j, byte) in hash.iter().enumerate() { body[offset + j] ^= byte; }
    }
    let trailer = body.iter().fold(CADENCE, |acc, byte| acc ^ byte);
    body.push(trailer);
    body
}

/// The composition an invocation's documents are written under: the base with the first
/// `depth − 1` layers, and its signature.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Composition {
    pub depth: usize,
    pub signature: Vec<u8>,
}

impl Composition {
    /// The base alone: what a daemon that publishes no acceptance is sent.
    pub fn base() -> Composition { Composition::of(&chain(), 1) }

    /// The first `depth` components of `hashes`.
    pub fn of(hashes: &[[u8; 32]], depth: usize) -> Composition {
        Composition { depth, signature: palimpsest(&hashes[..depth]) }
    }
}

/// Variant indices of `select Message`, in the schema's declaration order.
pub mod variant {
    pub const INIT: u64 = 0;
    pub const STDERR: u64 = 1;
    pub const CONTROL: u64 = 2;
    pub const SIGNAL: u64 = 3;
    pub const EXIT: u64 = 4;
    pub const VERIFY: u64 = 5;
    pub const SIGNAL_ACK: u64 = 6;
    pub const VERDICT: u64 = 7;
    pub const MODE: u64 = 8;
    pub const EXIT_STATUS: u64 = 9;
    pub const CLOSED: u64 = 10;
    // Sent by tooling, never by the launcher itself; listed so the indices stay complete.
    #[allow(dead_code)]
    pub const SHUTDOWN: u64 = 11;
}

/// The daemon reads documents from a peer it did not choose; so does the runner. A reply
/// larger than this is not a reply.
const MAXIMUM_LENGTH: u64 = 1 << 20;

// ── §4 varints ────────────────────────────────────────────────────────────────

pub fn encode_varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 0x80 {
        out.push(((n & 0x7f) as u8) | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

/// `(value, bytes consumed)`, or `None` for a truncated, over-wide or overlong encoding
/// (all B02 under §4).
pub fn decode_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let chunk = (b & 0x7f) as u64;
        if shift >= 64 || (shift == 63 && chunk > 1) { return None; }
        value |= chunk << shift;
        if b & 0x80 == 0 {
            if i > 0 && chunk == 0 { return None; }
            return Some((value, i + 1));
        }
        shift += 7;
    }
    None
}

// ── Encoding ──────────────────────────────────────────────────────────────────

/// The fields of one variant record, accumulated in declaration order (§7.2 canonical order
/// is member order, and every message here is written that way).
pub struct Record {
    count: u64,
    bytes: Vec<u8>,
}

impl Record {
    pub fn new() -> Record { Record { count: 0, bytes: Vec::new() } }

    pub fn scalar(&mut self, index: u64, text: &str) {
        encode_varint(&mut self.bytes, index);
        encode_varint(&mut self.bytes, text.len() as u64);
        self.bytes.extend_from_slice(text.as_bytes());
        self.count += 1;
    }

    pub fn flag(&mut self, index: u64) {
        encode_varint(&mut self.bytes, index);
        self.count += 1;
    }
}

/// A complete framed document (§6.1) carrying one `Message` of the given variant, written
/// under `composition`.
pub fn document(variant: u64, record: Record, composition: &Composition) -> Vec<u8> {
    let signature = &composition.signature;
    let mut body = Vec::with_capacity(record.bytes.len() + 8);
    encode_varint(&mut body, 1);             // root: one child, the select member
    encode_varint(&mut body, variant);       // the variant's keyword index
    encode_varint(&mut body, record.count);  // the record's child count
    body.extend_from_slice(&record.bytes);

    let mut signature_length = Vec::new();
    encode_varint(&mut signature_length, signature.len() as u64);
    let length = signature_length.len() + signature.len() + body.len();

    let mut out = Vec::with_capacity(4 + 2 + length);
    out.extend_from_slice(&MAGIC);
    encode_varint(&mut out, length as u64);
    out.extend_from_slice(&signature_length);
    out.extend_from_slice(signature);
    out.extend_from_slice(&body);
    out
}

// ── Decoding ──────────────────────────────────────────────────────────────────

/// A reply from the daemon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reply {
    SignalAck { accept: bool },
    Verdict { fresh: bool },
    Mode { canonical: bool },
    ExitStatus { code: i32 },
}

/// Reads exactly one framed document from `reader` — the magic number, the length varint and
/// then the declared bytes — and returns it whole. Nothing beyond the document is consumed.
pub fn read_document(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut out = vec![0u8; 4];
    reader.read_exact(&mut out)?;
    if out[..4] != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a BinTEL document"));
    }
    let mut declared: u64 = 0;
    let mut shift = 0;
    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte)?;
        out.push(byte[0]);
        if shift > 63 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "document length too wide"));
        }
        declared |= ((byte[0] & 0x7f) as u64) << shift;
        shift += 7;
        if byte[0] & 0x80 == 0 { break; }
    }
    if declared > MAXIMUM_LENGTH {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "document too long"));
    }
    let start = out.len();
    out.resize(start + declared as usize, 0);
    reader.read_exact(&mut out[start..])?;
    Ok(out)
}

/// The fields of the base's reply records: `(variant, kind)` for index 0 of each. The base
/// declares one member per reply record.
fn base_field_kind(variant: u64, index: u64) -> Option<Kind> {
    match (variant, index) {
        (variant::SIGNAL_ACK, 0) | (variant::VERDICT, 0) | (variant::MODE, 0) => Some(Kind::Flag),
        (variant::EXIT_STATUS, 0) => Some(Kind::Scalar),
        _ => None,
    }
}

/// How many members the base declares on a reply record.
fn base_field_count(variant: u64) -> u64 {
    match variant {
        variant::SIGNAL_ACK | variant::VERDICT | variant::MODE | variant::EXIT_STATUS => 1,
        _ => 0,
    }
}

/// The kind of field at `index` in a reply variant's record under a composition of the given
/// depth over `layers`: the base's members first, then those each layer appends, in order.
fn field_kind(layers: &[Layer], depth: usize, variant: u64, index: u64) -> Option<Kind> {
    if let Some(kind) = base_field_kind(variant, index) { return Some(kind); }
    let mut next = base_field_count(variant);
    for layer in layers.iter().take(depth.saturating_sub(1)) {
        for &(owner, kind) in layer.fields {
            if owner != variant { continue; }
            if next == index { return Some(kind); }
            next += 1;
        }
    }
    None
}

/// Decodes a framed reply document written under `composition`. `None` for anything that is
/// not a well-formed document of that composition carrying one reply variant.
pub fn parse_reply(document: &[u8], composition: &Composition) -> Option<Reply> {
    parse_reply_with(LAYERS, document, composition)
}

fn parse_reply_with(layers: &[Layer], document: &[u8], composition: &Composition) -> Option<Reply> {
    let signature = &composition.signature;
    if document.len() < 4 || document[..4] != MAGIC { return None; }
    let mut cur = 4;
    let (declared, n) = decode_varint(&document[cur..])?;
    cur += n;
    if declared as usize != document.len() - cur { return None; }

    let (signature_length, n) = decode_varint(&document[cur..])?;
    cur += n;
    if signature_length as usize != signature.len() { return None; }
    if document.len() < cur + signature.len() || document[cur..cur + signature.len()] != signature[..] {
        return None;
    }
    cur += signature.len();

    let (root_count, n) = decode_varint(&document[cur..])?;
    cur += n;
    if root_count != 1 { return None; }
    let (variant, n) = decode_varint(&document[cur..])?;
    cur += n;
    let (field_count, n) = decode_varint(&document[cur..])?;
    cur += n;

    let mut flags: Vec<u64> = Vec::new();
    let mut scalars: Vec<(u64, Vec<u8>)> = Vec::new();
    for _ in 0..field_count {
        let (index, n) = decode_varint(&document[cur..])?;
        cur += n;
        match field_kind(layers, composition.depth, variant, index)? {
            Kind::Flag => flags.push(index),
            Kind::Scalar => {
                let (length, n) = decode_varint(&document[cur..])?;
                cur += n;
                let end = cur.checked_add(length as usize)?;
                if end > document.len() { return None; }
                scalars.push((index, document[cur..end].to_vec()));
                cur = end;
            }
        }
    }
    // §6.1 field 2 / B16: the structure must end exactly where the declared length says.
    if cur != document.len() { return None; }

    let flag = |index: u64| flags.contains(&index);
    let text = |index: u64| -> Option<String> {
        scalars.iter().find(|(i, _)| *i == index)
            .and_then(|(_, bytes)| String::from_utf8(bytes.clone()).ok())
    };

    match variant {
        variant::SIGNAL_ACK => Some(Reply::SignalAck { accept: flag(0) }),
        variant::VERDICT => Some(Reply::Verdict { fresh: flag(0) }),
        variant::MODE => Some(Reply::Mode { canonical: flag(0) }),
        variant::EXIT_STATUS => Some(Reply::ExitStatus { code: text(0)?.trim().parse().ok()? }),
        _ => None,
    }
}

#[cfg(test)]
pub mod fixtures {
    use super::{Kind, Layer, variant, BASE};

    // Two invented layers, to exercise the composition machinery until the schema declares a
    // real one: the first appends a scalar to `Init` and a flag to `Mode`, the second another
    // flag to `Mode`. Their hashes share no four-byte prefix with each other or the base.
    pub static LAYERS: &[Layer] = &[
        Layer { hash: [0x11; 32], fields: &[(variant::INIT, Kind::Scalar), (variant::MODE, Kind::Flag)] },
        Layer { hash: [0x22; 32], fields: &[(variant::MODE, Kind::Flag)] },
    ];

    pub fn chain() -> Vec<[u8; 32]> {
        let mut hashes = vec![BASE];
        hashes.extend(LAYERS.iter().map(|layer| layer.hash));
        hashes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SIGNATURE: &str = "e50b7e82c11b06783dafa8a2ecc4e35f7ba31044ecd38fc5d9fe9e47a7c11e59e5";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn varints_round_trip_and_reject_overlong() {
        for n in [0u64, 1, 127, 128, 300, 16384, u64::MAX] {
            let mut out = Vec::new();
            encode_varint(&mut out, n);
            assert_eq!(decode_varint(&out), Some((n, out.len())));
        }
        assert_eq!(decode_varint(&[0x80, 0x00]), None);
        assert_eq!(decode_varint(&[0x80]), None);
    }

    // The base alone signs as its hash plus the cadence trailer: the constant the daemon's
    // tests pin ("the schema signature is pinned" in Soundness's `ethereal_test.scala`).
    #[test]
    fn the_base_signature_is_pinned() {
        assert_eq!(hex(&Composition::base().signature), SIGNATURE);
        assert_eq!(Composition::base().depth, 1);
        assert_eq!(Composition::base().signature.iter().fold(0u8, |a, b| a ^ b), CADENCE);
    }

    // §8.2's shapes: 33 bytes for one component, 37 for two, 39 for three; every byte XORs
    // to the cadence; the first four bytes are the base's, uncontested.
    #[test]
    fn palimpsests_have_the_pinned_shape() {
        let chain = fixtures::chain();
        for (depth, length) in [(1, 33), (2, 37), (3, 39)] {
            let signature = palimpsest(&chain[..depth]);
            assert_eq!(signature.len(), length);
            assert_eq!(signature.iter().fold(0u8, |a, b| a ^ b), CADENCE);
            assert_eq!(&signature[..4], &BASE[..4]);
        }
        // With the base XORed out, the second component's first two bytes sit at offset 4.
        let two = palimpsest(&chain[..2]);
        assert_eq!(two[4] ^ BASE[4], 0x11);
        assert_eq!(two[5] ^ BASE[5], 0x11);
    }

    #[test]
    fn exit_document_has_the_pinned_layout() {
        let mut record = Record::new();
        record.scalar(0, "42");
        let doc = document(variant::EXIT, record, &Composition::base());
        // magic, length (1 + 33 + 7 = 41), signature length, signature, body.
        assert_eq!(&doc[..4], &MAGIC);
        assert_eq!(doc[4], 41);
        assert_eq!(doc[5], 33);
        assert_eq!(hex(&doc[6..39]), SIGNATURE);
        assert_eq!(&doc[39..], &[0x01, 0x04, 0x01, 0x00, 0x02, b'4', b'2']);
    }

    // The frames the daemon's own tests pin (in Soundness, `ethereal_test.scala`'s "Launcher
    // protocol" suite),
    // produced by `Launcher.encode` on the Scala side: both implementations must agree
    // byte for byte.
    #[test]
    fn frames_match_the_daemon_side() {
        let sig = SIGNATURE;
        let base = Composition::base();
        let mut record = Record::new();
        record.scalar(0, "42");
        assert_eq!(hex(&document(variant::EXIT, record, &base)),
                   format!("b2c4b5bb2921{sig}01040100023432"));
        assert_eq!(hex(&document(variant::VERIFY, Record::new(), &base)),
                   format!("b2c4b5bb2521{sig}010500"));
        let mut record = Record::new();
        record.flag(0);
        assert_eq!(hex(&document(variant::MODE, record, &base)),
                   format!("b2c4b5bb2621{sig}01080100"));
        // stdout deliberately not a terminal while stdin and stderr are: `command > file`
        // run from a terminal. An all-true fixture would not catch the three flags being
        // written in the wrong order or under the wrong indices.
        let info = crate::protocol::ClientInfo {
            pid: 7, user_id: "501".into(), user_name: "jon".into(), script: "/usr/bin/x".into(),
            invoked_as: Some("x".into()), pwd: "/tmp".into(), args: vec!["a".into(), "b c".into()],
            env: vec!["K=V".into()], stdin_tty: true, stdout_tty: false, stderr_tty: true,
            umask: Some("022".into()), size: None, codepages: None,
        };
        // pid, uid, username, script, pwd; stdin-tty and stderr-tty flags (5, 7); the two
        // arguments (8); the environment (9); invoked-as (10); umask (11).
        assert_eq!(hex(&crate::protocol::init_document(&info, &base)),
                   format!("b2c4b5bb5b21{sig}01000c000137010335303102036a6f6e030a2f7573722f62696e2f7804042f746d700507080161080362206309034b3d560a01780b03303232"));

        // A WINCH carries the terminal's size (fields 2 and 3); a Windows close, its deadline.
        let detail = crate::protocol::SignalDetail { size: Some((80, 24)), deadline_ms: None };
        assert_eq!(hex(&crate::protocol::signal_document(7, "WINCH", detail, &base)),
                   format!("b2c4b5bb3721{sig}010304000137010557494e43480202383003023234"));
        let detail = crate::protocol::SignalDetail { size: None, deadline_ms: Some(5000) };
        assert_eq!(hex(&crate::protocol::signal_document(7, "CTRL_CLOSE", detail, &base)),
                   format!("b2c4b5bb3a21{sig}010303000137010a4354524c5f434c4f5345040435303030"));

        assert_eq!(hex(&crate::protocol::closed_document(7, "stdout", &base)),
                   format!("b2c4b5bb3021{sig}010a0200013701067374646f7574"));
    }

    // Under a deeper composition the frame carries the longer signature, and a layer's
    // field is readable at the index after the base's.
    #[test]
    fn frames_under_a_layered_composition_carry_its_signature() {
        let chain = fixtures::chain();
        let two = Composition::of(&chain, 2);
        let three = Composition::of(&chain, 3);

        let doc = document(variant::VERIFY, Record::new(), &two);
        assert_eq!(doc[5], 37);
        assert_eq!(&doc[6..43], &two.signature[..]);

        // Mode under the first fixture layer: base flag 0, layer flag 1.
        let mut record = Record::new();
        record.flag(0);
        record.flag(1);
        let doc = document(variant::MODE, record, &two);
        assert_eq!(parse_reply_with(fixtures::LAYERS, &doc, &two), Some(Reply::Mode { canonical: true }));
        // The same bytes are not a base document: wrong signature.
        assert_eq!(parse_reply_with(fixtures::LAYERS, &doc, &Composition::base()), None);
        // Nor a depth-3 one, whose signature is longer still.
        assert_eq!(parse_reply_with(fixtures::LAYERS, &doc, &three), None);

        // Index 2 exists only from the second layer on.
        let mut record = Record::new();
        record.flag(2);
        let doc = document(variant::MODE, record, &two);
        assert_eq!(parse_reply_with(fixtures::LAYERS, &doc, &two), None);
        let mut record = Record::new();
        record.flag(2);
        let doc = document(variant::MODE, record, &three);
        assert_eq!(parse_reply_with(fixtures::LAYERS, &doc, &three), Some(Reply::Mode { canonical: false }));
    }

    #[test]
    fn replies_parse() {
        let base = Composition::base();
        let mut record = Record::new();
        record.scalar(0, "3");
        let doc = document(variant::EXIT_STATUS, record, &base);
        assert_eq!(parse_reply(&doc, &base), Some(Reply::ExitStatus { code: 3 }));

        let mut record = Record::new();
        record.flag(0);
        let doc = document(variant::MODE, record, &base);
        assert_eq!(parse_reply(&doc, &base), Some(Reply::Mode { canonical: true }));

        let doc = document(variant::VERDICT, Record::new(), &base);
        assert_eq!(parse_reply(&doc, &base), Some(Reply::Verdict { fresh: false }));
    }

    #[test]
    fn read_document_consumes_exactly_one_document() {
        let base = Composition::base();
        let mut record = Record::new();
        record.flag(0);
        let mut stream = document(variant::SIGNAL_ACK, record, &base);
        let length = stream.len();
        stream.extend_from_slice(b"trailing");
        let mut cursor = std::io::Cursor::new(stream);
        let doc = read_document(&mut cursor).unwrap();
        assert_eq!(doc.len(), length);
        assert_eq!(cursor.position() as usize, length);
        assert_eq!(parse_reply(&doc, &base), Some(Reply::SignalAck { accept: true }));
    }

    #[test]
    fn a_foreign_signature_is_rejected() {
        let base = Composition::base();
        let mut record = Record::new();
        record.scalar(0, "3");
        let mut doc = document(variant::EXIT_STATUS, record, &base);
        doc[6] ^= 0x01;
        assert_eq!(parse_reply(&doc, &base), None);
    }
}
