fn main() {
    println!("Hello, world!");
}

// Player A
// nsec1nplkcc6tdgm4duh3ehc42kz7syrh7qzr9h9r9q5aqe8x8kuyqr9qtz24g0
// 6214f52a53639da8bb42b7306ddd4b4163a46711cf0f14bda0585715880107f1
// Player B
// nsec1tk2jzxtcex9um4gdz0elcnaejwrkwag5th3d6w3cce4ht7tj7xmsr9ydq8
// 2d8da9cfdb72cdedf2503d711eed06bc21c324b346b2a3b6436d7f7e60cb28dc

#[derive(Debug, thiserror::Error)]
pub enum AuctionError {
    #[error("Failed to decode hex {0}")]
    HexDecode(#[from] hex::FromHexError),
    #[error("Failed to convert integer {0}")]
    IntegerConversion(#[from] std::num::TryFromIntError),
    #[error("secp256k1 error {0}")]
    Secp256k1(#[from] bitcoin::secp256k1::Error),
}
pub static BTC_ESPLORA_CLIENT: std::sync::LazyLock<bdk_esplora::esplora_client::AsyncClient> =
    std::sync::LazyLock::new(|| {
        bdk_esplora::esplora_client::Builder::new("https://mutinynet.com/api")
            .build_async()
            .expect("Failed to create BTC Esplora client")
    });

const OP_CHECKSIGFROMSTACK: u8 = 0xcc;
static SECP: std::sync::LazyLock<bitcoin::secp256k1::Secp256k1<bitcoin::secp256k1::All>> =
    std::sync::LazyLock::new(bitcoin::secp256k1::Secp256k1::new);

fn build_bid_accepted_script(
    bid_accepted_id: &str,
    bid_accepted_pubkey: &str,
) -> Result<bitcoin::ScriptBuf, AuctionError> {
    let bid_accepted_id = hex::decode(bid_accepted_id)?;
    let bid_accepted_pubkey = hex::decode(bid_accepted_pubkey)?;
    let mut script_bytes = Vec::new();
    // push outcome message hash len + bytes(32 bytes)
    script_bytes.push(bid_accepted_id.len().try_into()?);
    script_bytes.extend_from_slice(bid_accepted_id.as_slice());

    // push oracle pubkey len + bytes (32 bytes)
    script_bytes.push(bid_accepted_pubkey.len().try_into()?);
    script_bytes.extend_from_slice(bid_accepted_pubkey.as_slice());

    // push OP_CHECKSIGFROMSTACK
    script_bytes.push(OP_CHECKSIGFROMSTACK);

    Ok(bitcoin::ScriptBuf::from_bytes(script_bytes))
}

fn nums_point() -> Result<bitcoin::XOnlyPublicKey, AuctionError> {
    let nums_bytes = [
        0x50, 0x92, 0x9b, 0x74, 0xc1, 0xa0, 0x49, 0x54, 0xb7, 0x8b, 0x4b, 0x60, 0x35, 0xe9, 0x7a,
        0x5e, 0x07, 0x8a, 0x5a, 0x0f, 0x28, 0xec, 0x96, 0xd5, 0x47, 0xbf, 0xee, 0x9a, 0xce, 0x80,
        0x3a, 0xc0,
    ];

    Ok(bitcoin::XOnlyPublicKey::from_slice(&nums_bytes)?)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::BTC_ESPLORA_CLIENT;

    static ALICE_KEYPAIR: std::sync::LazyLock<nostro2_signer::keypair::NostrKeypair> =
        std::sync::LazyLock::new(|| {
            let mut kp = "nsec1nplkcc6tdgm4duh3ehc42kz7syrh7qzr9h9r9q5aqe8x8kuyqr9qtz24g0"
                .parse::<nostro2_signer::keypair::NostrKeypair>()
                .expect("Failed to parse Alice's key");
            kp.set_extractable(true);
            kp
        });

    static ALICE_WALLET: std::sync::LazyLock<tokio::sync::RwLock<bdk_wallet::Wallet>> =
        std::sync::LazyLock::new(|| {
            let xprv: bitcoin::bip32::Xpriv = bitcoin::bip32::Xpriv::new_master(
                bitcoin::Network::Signet,
                ALICE_KEYPAIR
                    .mnemonic(nostro2_signer::Language::English)
                    .expect("Failed to get mnemonic")
                    .as_bytes(),
            )
            .unwrap();
            let (descriptor, _key_map, _) = bdk_wallet::template::DescriptorTemplate::build(
                bdk_wallet::descriptor::template::Bip86(xprv, bdk_wallet::KeychainKind::External),
                bitcoin::Network::Signet,
            )
            .expect("Failed to build external descriptor");

            let (change_descriptor, _change_key_map, _) =
                bdk_wallet::template::DescriptorTemplate::build(
                    bdk_wallet::descriptor::template::Bip86(
                        xprv,
                        bdk_wallet::KeychainKind::Internal,
                    ),
                    bitcoin::Network::Signet,
                )
                .expect("Failed to build internal descriptor");
            let wallet = bdk_wallet::Wallet::create(descriptor, change_descriptor)
                .network(bitcoin::Network::Signet)
                .create_wallet_no_persist()
                .expect("valid wallet");
            tokio::sync::RwLock::new(wallet)
        });

    static BOB_KEYPAIR: std::sync::LazyLock<nostro2_signer::keypair::NostrKeypair> =
        std::sync::LazyLock::new(|| {
            "nsec1tk2jzxtcex9um4gdz0elcnaejwrkwag5th3d6w3cce4ht7tj7xmsr9ydq8"
                .parse::<nostro2_signer::keypair::NostrKeypair>()
                .expect("Failed to parse Bob's key")
        });

    #[test]
    fn alice_and_bob_have_keys() {
        assert_eq!(ALICE_KEYPAIR.public_key().len(), 64);
        assert_eq!(BOB_KEYPAIR.public_key().len(), 64);
        assert_ne!(ALICE_KEYPAIR.public_key(), BOB_KEYPAIR.public_key());
        assert_eq!(
            "6214f52a53639da8bb42b7306ddd4b4163a46711cf0f14bda0585715880107f1",
            ALICE_KEYPAIR.public_key()
        );
        assert_eq!(
            "2d8da9cfdb72cdedf2503d711eed06bc21c324b346b2a3b6436d7f7e60cb28dc",
            BOB_KEYPAIR.public_key()
        );
    }

    #[test]
    fn alice_and_bob_can_sign_notes() {
        let mut note = nostro2::NostrNote {
            content: "test".to_string(),
            pubkey: ALICE_KEYPAIR.public_key().to_string(),
            ..Default::default()
        };
        ALICE_KEYPAIR
            .sign_note(&mut note)
            .expect("Failed to sign note");
        assert_eq!(note.pubkey, ALICE_KEYPAIR.public_key());
        assert_eq!(note.id.as_ref().map(|s| s.len()), Some(64));
        assert_eq!(note.sig.as_ref().map(|s| s.len()), Some(128));
        assert!(note.verify());
        // mutate the sig to ensure it fails if tampered with
        note.sig.replace("tampered".to_string());
        assert!(!note.verify());
        BOB_KEYPAIR
            .sign_note(&mut note)
            .expect("Failed to sign note");
        assert_eq!(note.pubkey, BOB_KEYPAIR.public_key());
        assert_eq!(note.id.as_ref().map(|s| s.len()), Some(64));
        assert_eq!(note.sig.as_ref().map(|s| s.len()), Some(128));
        assert!(note.verify());
    }

    #[test]
    fn create_outcome_scripts_for_alice() {
        let mut bid_accepted_template = nostro2::NostrNote {
            content: "bid accepted".to_string(),
            kind: 1,
            pubkey: ALICE_KEYPAIR.public_key().to_string(),
            // we need to keep a stable timestamp so the id doesnt change
            // could be the timestamp of the auction expiry or the bid creation time
            // for now we use 0 for simplicity
            created_at: 0,
            ..Default::default()
        };
        // serialize the Nostr id in Nostr format (last step is sha256 so we dont need to rehash it)
        bid_accepted_template
            .serialize_id()
            .expect("Failed to serialize id");
        assert_eq!(bid_accepted_template.id.as_ref().map(|s| s.len()), Some(64));
        let mut bid_accepted_template_reconstructed = nostro2::NostrNote {
            content: "bid accepted".to_string(),
            kind: 1,
            pubkey: ALICE_KEYPAIR.public_key().to_string(),
            // we need to keep a stable timestamp so the id doesnt change
            // could be the timestamp of the auction expiry or the bid creation time
            // for now we use 0 for simplicity
            created_at: 0,
            ..Default::default()
        };
        bid_accepted_template_reconstructed
            .serialize_id()
            .expect("Failed to serialize id");
        assert_eq!(
            bid_accepted_template_reconstructed
                .id
                .as_ref()
                .map(|s| s.len()),
            Some(64)
        );
        // ensure you can reconstruct the id from the agreed content
        assert_eq!(
            bid_accepted_template.id,
            bid_accepted_template_reconstructed.id
        );
        // a Nostr ID is already a sha256, so we dont need to rehash it
        let id = bid_accepted_template.id.expect("Failed to serialize id");
        println!("bid accepted id: {}", id);

        let script_buf = super::build_bid_accepted_script(&id, &bid_accepted_template.pubkey)
            .expect("Failed to build bid accepted script");
        let script_bytes = script_buf.to_bytes();

        println!("bid accepted script: {}", hex::encode(&script_bytes));
        assert_eq!(script_bytes.len(), 67);
        assert_eq!(script_bytes[0], 32);
        assert_eq!(script_bytes[33], 32);
        assert_eq!(script_bytes[66], super::OP_CHECKSIGFROMSTACK);

        println!("bid accepted script buf: {}", script_buf);
    }

    #[test]
    fn build_bid_address_script() {
        // First we create a bid accepted note for both Alice and Bob
        let mut alice_accepted_template = nostro2::NostrNote {
            content: "bid accepted".to_string(),
            kind: 1,
            pubkey: ALICE_KEYPAIR.public_key().to_string(),
            created_at: 0,
            ..Default::default()
        };
        alice_accepted_template
            .serialize_id()
            .expect("Failed to serialize id");
        assert_eq!(
            alice_accepted_template.id.as_ref().map(|s| s.len()),
            Some(64)
        );
        let mut bob_accepted_template = nostro2::NostrNote {
            content: "bid accepted".to_string(),
            kind: 1,
            pubkey: BOB_KEYPAIR.public_key().to_string(),
            created_at: 0,
            ..Default::default()
        };
        bob_accepted_template
            .serialize_id()
            .expect("Failed to serialize id");
        assert_eq!(bob_accepted_template.id.as_ref().map(|s| s.len()), Some(64));
        // and now we can build the scripts for both
        let alice_script = super::build_bid_accepted_script(
            &alice_accepted_template.id.expect("Failed to serialize id"),
            &alice_accepted_template.pubkey,
        )
        .expect("Failed to build bid accepted script");
        let bob_script = super::build_bid_accepted_script(
            &bob_accepted_template.id.expect("Failed to serialize id"),
            &bob_accepted_template.pubkey,
        )
        .expect("Failed to build bid accepted script");
        println!("alice script: {alice_script}");
        println!("bob script: {bob_script}");

        // we are creating a taproot address with two spend paths
        //
        // Path 0: CSFS verification for alice
        // Path 1: CSFS verification for bob
        let nums_point = super::nums_point().expect("Failed to get nums point");
        println!("nums point: {nums_point}");
        let bid_address = bitcoin::taproot::TaprootBuilder::new()
            .add_leaf(1, alice_script)
            .expect("Failed to add leaf")
            .add_leaf(1, bob_script)
            .expect("Failed to add leaf")
            .finalize(&super::SECP, nums_point)
            .expect("Failed to finalize taproot");
        let address =
            bitcoin::Address::p2tr_tweaked(bid_address.output_key(), bitcoin::Network::Signet);
        println!("bid address: {address}");
        assert_eq!(
            address.to_string(),
            "tb1pd790gwtaajsd5wzy3jc6dlw4yf97mrpaz77mjnumm0fequexj3fq0jnpv5"
        );
    }
    #[tokio::test]
    async fn alice_can_check_her_wallet() {
        let mut wallet = ALICE_WALLET.write().await;
        let balance = wallet.balance();
        println!("Alice's balance: {balance}");
        let full_scan_request = wallet.start_full_scan();
        let update = bdk_esplora::EsploraAsyncExt::full_scan(
            &*super::BTC_ESPLORA_CLIENT,
            full_scan_request,
            4,
            4,
        )
        .await
        .unwrap();

        // Apply the update from the full scan to the wallet
        wallet.apply_update(update).unwrap();

        let new_balance = wallet.balance();
        println!("Alice's new balance: {new_balance}");
    }
    #[tokio::test]
    async fn bid_address_is_funded() {
        let address = bitcoin::Address::from_str(
            "tb1pd790gwtaajsd5wzy3jc6dlw4yf97mrpaz77mjnumm0fequexj3fq0jnpv5",
        )
        .unwrap();
        let checked_address = address
            .require_network(bitcoin::Network::Signet)
            .expect("valid address");

        let txs = super::BTC_ESPLORA_CLIENT
            .get_address_stats(&checked_address)
            .await
            .unwrap();
        println!("txs: {txs:?}");

        assert!(txs.chain_stats.funded_txo_sum > 0);
    }
    #[tokio::test]
    async fn alice_can_spend_bid() {
        let mut alice_accepted_template = nostro2::NostrNote {
            content: "bid accepted".to_string(),
            kind: 1,
            pubkey: ALICE_KEYPAIR.public_key().to_string(),
            created_at: 0,
            ..Default::default()
        };
        alice_accepted_template
            .serialize_id()
            .expect("Failed to serialize id");
        assert_eq!(
            alice_accepted_template.id.as_ref().map(|s| s.len()),
            Some(64)
        );
        let mut bob_accepted_template = nostro2::NostrNote {
            content: "bid accepted".to_string(),
            kind: 1,
            pubkey: BOB_KEYPAIR.public_key().to_string(),
            created_at: 0,
            ..Default::default()
        };
        bob_accepted_template
            .serialize_id()
            .expect("Failed to serialize id");
        assert_eq!(bob_accepted_template.id.as_ref().map(|s| s.len()), Some(64));
        // and now we can build the scripts for both
        let alice_script = super::build_bid_accepted_script(
            alice_accepted_template
                .id
                .as_ref()
                .expect("Failed to serialize id"),
            &alice_accepted_template.pubkey,
        )
        .expect("Failed to build bid accepted script");
        let bob_script = super::build_bid_accepted_script(
            &bob_accepted_template.id.expect("Failed to serialize id"),
            &bob_accepted_template.pubkey,
        )
        .expect("Failed to build bid accepted script");
        println!("alice script: {alice_script}");
        println!("bob script: {bob_script}");

        // we are creating a taproot address with two spend paths
        //
        // Path 0: CSFS verification for alice
        // Path 1: CSFS verification for bob
        let nums_point = super::nums_point().expect("Failed to get nums point");
        println!("nums point: {nums_point}");
        let bid_address = bitcoin::taproot::TaprootBuilder::new()
            .add_leaf(1, alice_script.clone())
            .expect("Failed to add leaf")
            .add_leaf(1, bob_script)
            .expect("Failed to add leaf")
            .finalize(&super::SECP, nums_point)
            .expect("Failed to finalize taproot");

        let tapleaf = bitcoin::TapLeafHash::from_script(
            alice_script.as_script(),
            bitcoin::taproot::LeafVersion::TapScript,
        );

        println!("alice_script: {alice_script}");
        println!("tapleaf: {tapleaf}");

        // 3) Compute the control block for this exact leaf.
        let ctrl = bid_address
            .control_block(&(
                alice_script.clone(),
                bitcoin::taproot::LeafVersion::TapScript,
            ))
            .expect("failed to compute control block");

        // Sign the note with Alice's key, will add the signature to the note
        ALICE_KEYPAIR
            .sign_note(&mut alice_accepted_template)
            .unwrap();
        // Build witness: [<alice_signatue> <message>  <script> <control_block>]
        let mut wit = bitcoin::Witness::new();
        wit.push(
            hex::decode(alice_accepted_template.sig.as_ref().unwrap())
                .unwrap()
                .as_slice(),
        );
        wit.push(
            hex::decode(alice_accepted_template.id.as_ref().unwrap())
                .unwrap()
                .as_slice(),
        );
        wit.push(alice_script.as_bytes());
        wit.push(ctrl.serialize());

        let address =
            bitcoin::Address::p2tr_tweaked(bid_address.output_key(), bitcoin::Network::Signet);
        let address_utxos = BTC_ESPLORA_CLIENT
            .get_address_txs(&address, None)
            .await
            .unwrap();

        // TODO: we need to validate the outpoint is actually spendable
        let funded_utxo = address_utxos.first().unwrap().txid;

        // an address to send the money to
        let to_address = ALICE_WALLET
            .write()
            .await
            .next_unused_address(bdk_wallet::KeychainKind::External);

        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: bitcoin::locktime::absolute::LockTime::ZERO,
            input: vec![bitcoin::TxIn {
                sequence: bitcoin::blockdata::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
                previous_output: bitcoin::OutPoint {
                    txid: funded_utxo,
                    vout: 1,
                },
                witness: wit,
                ..Default::default()
            }],
            output: vec![bitcoin::TxOut {
                // TODO: spend the whole amount
                value: bitcoin::Amount::from_sat(1000),
                script_pubkey: to_address.script_pubkey(),
            }],
        };
        BTC_ESPLORA_CLIENT.broadcast(&tx).await.unwrap();
    }
}
