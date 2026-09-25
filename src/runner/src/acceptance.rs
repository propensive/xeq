//! The daemon's acceptance (BinTEL §8.4): the file `acceptance` in the state directory, in
//! which the daemon names, in preference order, the compositions of the launcher schema it
//! can read and the further layers it holds. The launcher reads it before its first
//! connection and writes every document of the invocation under the richest composition it
//! can serve — the writer obligations of §8.4, specialised to a library that is a single
//! chain of prefixes (`bintel::LAYERS`), so that no palimpsest search is needed: an
//! alternative's `schema` is servable iff it is byte-for-byte the signature of some prefix of
//! the chain. See `spec/layout.md` and `spec/launcher.md`.
//!
//! The file is the bare form of the message — the document root alone, under the
//! `acceptance` schema, whose keyword order is compiled in below — so parsing it is the same
//! varint-and-node walk as a reply, with no framing.

use std::path::Path;

use crate::bintel::{decode_varint, palimpsest, Composition};

/// One alternative of an acceptance: the composition the daemon requires, as a signature; the
/// two flags; and the further components it holds, each a hash or a prefix of one.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Alternative {
    pub schema: Vec<u8>,
    pub self_contained: bool,
    pub any_published: bool,
    pub components: Vec<Vec<u8>>,
}

// The `acceptance` schema's keyword order: the root has one member, `accept` (0), a record
// whose members are `schema` (0), `self-contained` (1), `any-published` (2) and `component`
// (3).
const ACCEPT: u64 = 0;
const SCHEMA: u64 = 0;
const SELF_CONTAINED: u64 = 1;
const ANY_PUBLISHED: u64 = 2;
const COMPONENT: u64 = 3;

/// An acceptance naming a few dozen components is under a kilobyte; the file is written by
/// the daemon, but read by a launcher that should not trust it to be small.
pub const MAXIMUM_LENGTH: usize = 64 * 1024;

/// Parses the bare form. `None` for anything that is not structurally an acceptance: a
/// member the schema does not declare, a component shorter than the four bytes the schema's
/// pattern admits, or bytes left over. Codec-level checks — that a `schema` is structurally a
/// signature — are left to `choose`, which compares it against signatures it computed itself.
pub fn parse(bytes: &[u8]) -> Option<Vec<Alternative>> {
    if bytes.len() > MAXIMUM_LENGTH { return None; }
    let mut cur = 0;
    let (root_count, n) = decode_varint(&bytes[cur..])?;
    cur += n;
    let mut alternatives = Vec::new();
    for _ in 0..root_count {
        let (index, n) = decode_varint(&bytes[cur..])?;
        cur += n;
        if index != ACCEPT { return None; }
        let (field_count, n) = decode_varint(&bytes[cur..])?;
        cur += n;
        let mut alternative = Alternative::default();
        for _ in 0..field_count {
            let (index, n) = decode_varint(&bytes[cur..])?;
            cur += n;
            match index {
                SELF_CONTAINED => alternative.self_contained = true,
                ANY_PUBLISHED => alternative.any_published = true,
                SCHEMA | COMPONENT => {
                    let (length, n) = decode_varint(&bytes[cur..])?;
                    cur += n;
                    let end = cur.checked_add(length as usize)?;
                    if end > bytes.len() { return None; }
                    let value = bytes[cur..end].to_vec();
                    cur = end;
                    if index == SCHEMA { alternative.schema = value; }
                    else {
                        if value.len() < 4 || value.len() > 32 { return None; }
                        alternative.components.push(value);
                    }
                }
                _ => return None,
            }
        }
        if alternative.schema.is_empty() { return None; }
        alternatives.push(alternative);
    }
    if cur != bytes.len() || alternatives.is_empty() { return None; }
    Some(alternatives)
}

/// How many components a signature of `length` bytes names (§8.2 decoding step 1), or `None`
/// for a length no signature has.
fn component_count(length: usize) -> Option<usize> {
    if length == 33 { Some(1) }
    else if length >= 37 && (length - 37) % 2 == 0 { Some(2 + (length - 37) / 2) }
    else { None }
}

/// Whether `prefix` denotes exactly one component of `chain` — the one at `position` — under
/// §8.4: a value matching no component denotes nothing, and so does one matching several.
fn denotes(prefix: &[u8], chain: &[[u8; 32]], position: usize) -> bool {
    let matches = |hash: &[u8; 32]| hash.starts_with(prefix);
    matches(&chain[position]) && chain.iter().filter(|hash| matches(hash)).count() == 1
}

/// The first alternative the launcher can serve, as the depth of the composition to write
/// under (§8.4 writer obligations): the alternative's `schema` must be a prefix of `chain`,
/// and the composition is extended by each further layer of the chain, in order, for as long
/// as the alternative names it — by a prefix of its hash, or wholesale by `any-published`,
/// every layer of the specification being a published component. `None` when no alternative
/// is servable, which means the daemon speaks another base or requires a layer this launcher
/// lacks.
pub fn choose(alternatives: &[Alternative], chain: &[[u8; 32]]) -> Option<usize> {
    for alternative in alternatives {
        let Some(k) = component_count(alternative.schema.len()) else { continue };
        if k > chain.len() || palimpsest(&chain[..k]) != alternative.schema { continue; }
        let mut depth = k;
        while depth < chain.len() {
            let named = alternative.any_published
                || alternative.components.iter().any(|component| denotes(component, chain, depth));
            if !named { break; }
            depth += 1;
        }
        return Some(depth);
    }
    None
}

/// Why no composition could be chosen: the base the daemon's first alternative names, for
/// the message the launcher prints.
#[derive(Debug, Eq, PartialEq)]
pub struct Mismatch {
    pub daemon_base: Vec<u8>,
}

/// The composition to write under, from the daemon's acceptance file. A missing or malformed
/// file means a daemon that publishes nothing — one that predates acceptances — and the base
/// alone is what such a daemon reads.
pub fn load(acceptance_file: &Path) -> Result<Composition, Mismatch> {
    let chain = crate::bintel::chain();
    match std::fs::read(acceptance_file).ok().and_then(|bytes| parse(&bytes)) {
        None => Ok(Composition::base()),
        Some(alternatives) => match choose(&alternatives, &chain) {
            Some(depth) => Ok(Composition::of(&chain, depth)),
            None => Err(Mismatch {
                daemon_base: alternatives[0].schema.iter().copied().take(32).collect(),
            }),
        },
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bintel::{encode_varint, fixtures, BASE};

    // Writes an acceptance in the bare form, as the daemon does.
    fn acceptance(alternatives: &[Alternative]) -> Vec<u8> {
        let mut out = Vec::new();
        encode_varint(&mut out, alternatives.len() as u64);
        for alternative in alternatives {
            encode_varint(&mut out, ACCEPT);
            let count = 1 + alternative.self_contained as u64 + alternative.any_published as u64
                + alternative.components.len() as u64;
            encode_varint(&mut out, count);
            encode_varint(&mut out, SCHEMA);
            encode_varint(&mut out, alternative.schema.len() as u64);
            out.extend_from_slice(&alternative.schema);
            if alternative.self_contained { encode_varint(&mut out, SELF_CONTAINED); }
            if alternative.any_published { encode_varint(&mut out, ANY_PUBLISHED); }
            for component in &alternative.components {
                encode_varint(&mut out, COMPONENT);
                encode_varint(&mut out, component.len() as u64);
                out.extend_from_slice(component);
            }
        }
        out
    }

    fn alternative(chain: &[[u8; 32]], depth: usize, components: &[&[u8]]) -> Alternative {
        Alternative {
            schema: palimpsest(&chain[..depth]),
            components: components.iter().map(|c| c.to_vec()).collect(),
            ..Alternative::default()
        }
    }

    // The 38 bytes a daemon holding the base and no layer writes: one alternative, the base
    // alone. The daemon's tests in Soundness pin the same bytes.
    #[test]
    fn the_base_only_acceptance_is_pinned() {
        let chain = crate::bintel::chain();
        let bytes = acceptance(&[alternative(&chain, 1, &[])]);
        assert_eq!(bytes.len(), 38);
        assert_eq!(hex(&bytes), format!("0100010021{}", hex(&Composition::base().signature)));
        let parsed = parse(&bytes).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].schema, Composition::base().signature);
        assert_eq!(choose(&parsed, &chain), Some(1));
    }

    #[test]
    fn layers_are_taken_in_chain_order_when_named() {
        let chain = fixtures::chain();
        let l1 = &chain[1][..4];
        let l2 = &chain[2][..4];
        // Both named, by four-byte prefix: the whole chain.
        assert_eq!(choose(&[alternative(&chain, 1, &[l1, l2])], &chain), Some(3));
        // Named in the other order: still the whole chain, since components are a set.
        assert_eq!(choose(&[alternative(&chain, 1, &[l2, l1])], &chain), Some(3));
        // Only the first: base with one layer.
        assert_eq!(choose(&[alternative(&chain, 1, &[l1])], &chain), Some(2));
        // Only the second: the chain cannot skip a layer, so the base alone.
        assert_eq!(choose(&[alternative(&chain, 1, &[l2])], &chain), Some(1));
        // A requirement that already includes the first layer, then the second named.
        assert_eq!(choose(&[alternative(&chain, 2, &[l2])], &chain), Some(3));
        // A layer this launcher does not have denotes nothing.
        assert_eq!(choose(&[alternative(&chain, 1, &[&[0x33, 0x33, 0x33, 0x33]])], &chain), Some(1));
        // A full hash names a layer too.
        assert_eq!(choose(&[alternative(&chain, 1, &[&chain[1][..]])], &chain), Some(2));
    }

    #[test]
    fn any_published_names_every_layer() {
        let chain = fixtures::chain();
        let mut alt = alternative(&chain, 1, &[]);
        alt.any_published = true;
        assert_eq!(choose(&[alt], &chain), Some(3));
    }

    #[test]
    fn an_ambiguous_prefix_denotes_nothing() {
        // Two layers whose hashes share their first four bytes.
        let chain = [BASE, [0x44; 32], { let mut h = [0x44; 32]; h[4] = 0x55; h }];
        assert_eq!(choose(&[alternative(&chain, 1, &[&[0x44; 4]])], &chain), Some(1));
        // Five bytes tell them apart.
        assert_eq!(choose(&[alternative(&chain, 1, &[&[0x44; 5]])], &chain), Some(2));
    }

    #[test]
    fn alternatives_are_tried_in_order() {
        let chain = fixtures::chain();
        let foreign = alternative(&[[0x99; 32]], 1, &[]);
        // A foreign base first, then ours: the second is served.
        assert_eq!(choose(&[foreign.clone(), alternative(&chain, 2, &[])], &chain), Some(2));
        // A requirement deeper than the chain cannot be served.
        let deeper = alternative(&[BASE, [0x11; 32], [0x22; 32], [0x33; 32]], 4, &[]);
        assert_eq!(choose(&[deeper], &chain), None);
        // Only foreign bases: nothing.
        assert_eq!(choose(&[foreign], &chain), None);
        // The self-contained flag changes nothing for a launcher, which cannot read one.
        let mut alt = alternative(&chain, 1, &[]);
        alt.self_contained = true;
        assert_eq!(choose(&[alt], &chain), Some(1));
    }

    #[test]
    fn malformed_acceptances_are_rejected() {
        let chain = fixtures::chain();
        let good = acceptance(&[alternative(&chain, 1, &[&chain[1][..4]])]);
        assert!(parse(&good).is_some());
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&[0x00]), None);                         // no alternatives
        assert_eq!(parse(&good[..good.len() - 1]), None);         // truncated
        let mut trailing = good.clone(); trailing.push(0x00);
        assert_eq!(parse(&trailing), None);                       // bytes left over
        let mut wrong_member = good.clone(); wrong_member[1] = 0x01;
        assert_eq!(parse(&wrong_member), None);                   // root member 1: undeclared
        // A component of three bytes is outside the schema's pattern.
        let mut short = good.clone();
        let component_at = good.len() - 6;
        short[component_at + 1] = 3; short.truncate(component_at + 5);
        assert_eq!(parse(&short), None);
        assert_eq!(parse(&vec![0x01; MAXIMUM_LENGTH + 1]), None);
        // A schema of a length no signature has is parsed but never chosen.
        let mut odd = alternative(&chain, 1, &[]);
        odd.schema.push(0);
        assert_eq!(choose(&[odd], &chain), None);
    }

    #[test]
    fn a_file_is_read_and_its_absence_means_the_base() {
        let dir = std::env::temp_dir().join(format!("xeq-acceptance-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("acceptance");
        assert_eq!(load(&file), Ok(Composition::base()));
        std::fs::write(&file, b"garbage").unwrap();
        assert_eq!(load(&file), Ok(Composition::base()));
        let chain = crate::bintel::chain();
        std::fs::write(&file, acceptance(&[alternative(&chain, 1, &[])])).unwrap();
        assert_eq!(load(&file), Ok(Composition::base()));
        std::fs::write(&file, acceptance(&[alternative(&[[0x99; 32]], 1, &[])])).unwrap();
        assert_eq!(load(&file), Err(Mismatch { daemon_base: vec![0x99; 32] }));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
