use std::{path::PathBuf, str::FromStr};

use base64::Engine;
use rand_core::{CryptoRng, RngCore};
use serde_json::Value;
use slnt_sdk::{
    keys::{
        canonical_message, derive_stealth_keys, derive_stealth_keys_hd, label_tweak_scalar,
        MetaAddress, Network, SCHEME_ID_V1,
    },
    pinboard::{build_post_instruction, try_parse_note_log, NoteEvent, NOTE_EVENT_DISCRIMINATOR},
    recipient::scan_note_candidates,
    registry::{
        build_register_instruction, registry_pda, MetaAddressEntry, MetaAddressPayload,
        META_ADDRESS_ENTRY_DISCRIMINATOR,
    },
    sender::derive_payment,
};
use solana_sdk::pubkey::Pubkey;

struct FixedRng {
    bytes: [u8; 32],
}

impl FixedRng {
    fn new(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }
}

impl RngCore for FixedRng {
    fn next_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes[0..4].try_into().unwrap())
    }

    fn next_u64(&mut self) -> u64 {
        u64::from_le_bytes(self.bytes[0..8].try_into().unwrap())
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        assert_eq!(
            dest.len(),
            32,
            "SLNT sender requests exactly 32 random bytes"
        );
        dest.copy_from_slice(&self.bytes);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for FixedRng {}

fn vectors() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-vectors.json");
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&json).expect("test-vectors.json parses")
}

fn hex_bytes(s: &str) -> Vec<u8> {
    hex::decode(s.strip_prefix("0x").unwrap_or(s)).expect("valid hex")
}

fn hex_array<const N: usize>(s: &str) -> [u8; N] {
    hex_bytes(s).try_into().unwrap_or_else(|v: Vec<u8>| {
        panic!("expected {N} bytes, got {}", v.len());
    })
}

fn view_tag(s: &str) -> u8 {
    u8::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).expect("valid view-tag hex")
}

fn network(s: &str) -> Network {
    match s {
        "Mainnet" => Network::Mainnet,
        "Devnet" => Network::Devnet,
        "Testnet" => Network::Testnet,
        "Localnet" => Network::Localnet,
        other => panic!("unknown network {other}"),
    }
}

#[test]
fn key_derivation_and_meta_address_vectors_match() {
    let root = vectors();
    assert_eq!(root["srfc"], "sRFC-0042");
    assert_eq!(root["version"], 1);

    for case in root["vectors"]["method1_hd"].as_array().unwrap() {
        let seed = hex_bytes(case["seed_hex"].as_str().unwrap());
        let account = case["account"].as_u64().unwrap() as u32;
        let (spend, scan) = derive_stealth_keys_hd(&seed, account).unwrap();
        let meta = MetaAddress::from_keys(&spend, &scan);

        assert_eq!(hex::encode(spend.public_bytes()), case["b_spend_hex"]);
        assert_eq!(hex::encode(scan.public_bytes()), case["b_scan_hex"]);
        assert_eq!(meta.encode_bech32m().unwrap(), case["meta_address"]);
    }

    for case in root["vectors"]["method2_signature"].as_array().unwrap() {
        let sig = hex_array::<64>(case["signature_hex"].as_str().unwrap());
        let (spend, scan) = derive_stealth_keys(&sig).unwrap();
        let meta = MetaAddress::from_keys(&spend, &scan);

        assert_eq!(
            canonical_message(network(case["network"].as_str().unwrap())),
            case["canonical_message_utf8"].as_str().unwrap()
        );
        assert_eq!(hex::encode(spend.public_bytes()), case["b_spend_hex"]);
        assert_eq!(hex::encode(scan.raw), case["b_scan_raw_hex"]);
        assert_eq!(hex::encode(scan.public_bytes()), case["b_scan_hex"]);
        assert_eq!(meta.encode_bech32m().unwrap(), case["meta_address"]);
    }

    for case in root["vectors"]["labels"].as_array().unwrap() {
        let sig = hex_array::<64>(case["signature_hex"].as_str().unwrap());
        let label_index = case["label_index"].as_u64().unwrap() as u32;
        let (spend, scan) = derive_stealth_keys(&sig).unwrap();
        let meta = MetaAddress::for_label(&spend, &scan, label_index);
        let tweak = label_tweak_scalar(&scan, label_index);

        assert_eq!(hex::encode(tweak.to_bytes()), case["label_tweak_hex"]);
        assert_eq!(hex::encode(meta.b_spend), case["b_spend_hex"]);
        assert_eq!(hex::encode(meta.b_scan), case["b_scan_hex"]);
        assert_eq!(meta.encode_bech32m().unwrap(), case["meta_address"]);
    }
}

#[test]
fn payment_and_recipient_scan_vectors_match() {
    let root = vectors();

    for case in root["vectors"]["sender_derivation"].as_array().unwrap() {
        let meta = MetaAddress::decode_bech32m(case["meta_address"].as_str().unwrap()).unwrap();
        let seed = hex_array::<32>(case["ephemeral_secret_hex"].as_str().unwrap());
        let mut rng = FixedRng::new(seed);
        let payment = derive_payment(&meta, &mut rng).unwrap();

        assert_eq!(payment.stealth_address.to_string(), case["stealth_address"]);
        assert_eq!(
            hex::encode(payment.stealth_address.to_bytes()),
            case["stealth_address_hex"]
        );
        assert_eq!(
            hex::encode(payment.ephemeral_pub),
            case["ephemeral_pub_hex"]
        );
        assert_eq!(format!("0x{:02x}", payment.view_tag), case["view_tag_hex"]);
    }

    for case in root["vectors"]["recipient_scan"].as_array().unwrap() {
        let sig = hex_array::<64>(case["signature_hex"].as_str().unwrap());
        let (spend, scan) = derive_stealth_keys(&sig).unwrap();
        let known_labels: Vec<u32> = case["known_labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        let matches = scan_note_candidates(
            &spend,
            &scan,
            &hex_array::<32>(case["ephemeral_pub_hex"].as_str().unwrap()),
            view_tag(case["view_tag_hex"].as_str().unwrap()),
            &known_labels,
        )
        .unwrap();
        let expected = case["matches"].as_array().unwrap();
        assert_eq!(matches.len(), expected.len());
        for (actual, expected) in matches.iter().zip(expected) {
            assert_eq!(
                actual.label_index,
                expected["label_index"].as_u64().unwrap() as u32
            );
            assert_eq!(
                actual.stealth_address.to_string(),
                expected["stealth_address"]
            );
            assert_eq!(
                hex::encode(actual.stealth_scalar.to_bytes()),
                expected["stealth_scalar_hex"]
            );
        }
    }
}

#[test]
fn pinboard_and_registry_wire_vectors_match() {
    let root = vectors();
    let pinboard = &root["vectors"]["pinboard"]["note_event"];
    let event = NoteEvent {
        scheme_id: pinboard["scheme_id"].as_u64().unwrap() as u16,
        ephemeral_pub: hex_array::<32>(pinboard["ephemeral_pub_hex"].as_str().unwrap()),
        view_tag: view_tag(pinboard["view_tag_hex"].as_str().unwrap()),
        metadata: hex_bytes(pinboard["metadata_hex"].as_str().unwrap()),
    };
    let mut body = Vec::new();
    borsh::to_writer(&mut body, &event).unwrap();
    let mut payload = NOTE_EVENT_DISCRIMINATOR.to_vec();
    payload.extend_from_slice(&body);

    assert_eq!(event.scheme_id, SCHEME_ID_V1);
    assert_eq!(
        hex::encode(NOTE_EVENT_DISCRIMINATOR),
        pinboard["event_discriminator_hex"]
    );
    assert_eq!(hex::encode(&body), pinboard["borsh_body_hex"]);
    assert_eq!(hex::encode(&payload), pinboard["event_payload_hex"]);
    assert_eq!(
        base64::engine::general_purpose::STANDARD.encode(&payload),
        pinboard["program_data_base64"]
    );
    assert_eq!(
        try_parse_note_log(&format!(
            "Program data: {}",
            pinboard["program_data_base64"].as_str().unwrap()
        ))
        .unwrap()
        .unwrap(),
        event
    );

    let post_ix = build_post_instruction(
        &Pubkey::from_str(pinboard["program_id"].as_str().unwrap()).unwrap(),
        &Pubkey::from_str(pinboard["fee_payer"].as_str().unwrap()).unwrap(),
        event.scheme_id,
        event.ephemeral_pub,
        event.view_tag,
        event.metadata,
    );
    assert_eq!(
        hex::encode(post_ix.data),
        pinboard["post_instruction_data_hex"]
    );

    let registry = &root["vectors"]["registry"]["register"];
    let program_id = Pubkey::from_str(registry["program_id"].as_str().unwrap()).unwrap();
    let registrant = Pubkey::from_str(registry["registrant"].as_str().unwrap()).unwrap();
    let scheme_id = registry["scheme_id"].as_u64().unwrap() as u16;
    let payload = MetaAddressPayload {
        version: registry["payload"]["version"].as_u64().unwrap() as u8,
        b_spend: hex_array::<32>(registry["payload"]["b_spend_hex"].as_str().unwrap()),
        b_scan: hex_array::<32>(registry["payload"]["b_scan_hex"].as_str().unwrap()),
        flags: registry["payload"]["flags"].as_u64().unwrap() as u8,
    };
    let (pda, bump) = registry_pda(&program_id, &registrant, scheme_id);
    assert_eq!(pda.to_string(), registry["pda"]);
    assert_eq!(bump, registry["bump"].as_u64().unwrap() as u8);

    let register_ix =
        build_register_instruction(&program_id, &registrant, scheme_id, payload.clone());
    assert_eq!(
        hex::encode(register_ix.data),
        registry["instruction_data_hex"]
    );

    let entry = MetaAddressEntry {
        registrant,
        scheme_id,
        bump,
        version: payload.version,
        b_spend: payload.b_spend,
        b_scan: payload.b_scan,
        flags: payload.flags,
    };
    let mut account_data = META_ADDRESS_ENTRY_DISCRIMINATOR.to_vec();
    borsh::to_writer(&mut account_data, &entry).unwrap();
    assert_eq!(
        hex::encode(META_ADDRESS_ENTRY_DISCRIMINATOR),
        registry["account_discriminator_hex"]
    );
    assert_eq!(hex::encode(account_data), registry["account_data_hex"]);
}

#[test]
fn invalid_hardening_vectors_are_rejected() {
    let root = vectors();
    let invalid = &root["vectors"]["invalid"];
    let base_meta =
        MetaAddress::decode_bech32m(invalid["base_meta_address"].as_str().unwrap()).unwrap();
    let r = hex_array::<32>(invalid["ephemeral_secret_hex"].as_str().unwrap());

    let mut identity = base_meta.clone();
    identity.b_spend = hex_array::<32>(invalid["bad_spend_identity_hex"].as_str().unwrap());
    assert!(matches!(
        derive_payment(&identity, &mut FixedRng::new(r)),
        Err(slnt_sdk::SlntError::InvalidPoint)
    ));

    let mut torsion = base_meta.clone();
    torsion.b_spend = hex_array::<32>(invalid["bad_spend_with_torsion_hex"].as_str().unwrap());
    assert!(matches!(
        derive_payment(&torsion, &mut FixedRng::new(r)),
        Err(slnt_sdk::SlntError::InvalidPoint)
    ));

    let mut low_order_scan = base_meta;
    low_order_scan.b_scan = hex_array::<32>(invalid["bad_scan_low_order_hex"].as_str().unwrap());
    assert!(matches!(
        derive_payment(&low_order_scan, &mut FixedRng::new(r)),
        Err(slnt_sdk::SlntError::InvalidSharedSecret)
    ));
}
