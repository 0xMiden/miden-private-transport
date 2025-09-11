use miden_objects::note::NoteHeader;
use miden_private_transport_client::types::test_note_header;
use rand::Rng;

const DETAILS_LEN_AVG: usize = 1500;
const DETAILS_LEN_DEV: usize = 100;
pub const TAG_LOCAL_ANY: u32 = 0xc000_0000;

pub enum TagGeneration {
    Sequential(u32),
    Random,
}

pub fn generate_dummy_notes(n: usize, tag_gen: &TagGeneration) -> Vec<(NoteHeader, Vec<u8>)> {
    let mut rng = rand::rng();

    let mut tag = TAG_LOCAL_ANY;
    (0..n)
        .map(|_| {
            tag = match tag_gen {
                TagGeneration::Sequential(offset) => tag + 1 + offset,
                TagGeneration::Random => TAG_LOCAL_ANY + rng.random_range(0..(1 << 29)),
            };
            let header = test_note_header(tag.into());
            let details = vec![
                0u8;
                DETAILS_LEN_AVG
                    + rng.random_range(0..(DETAILS_LEN_DEV * 2 - DETAILS_LEN_DEV))
            ];
            (header, details)
        })
        .collect()
}
